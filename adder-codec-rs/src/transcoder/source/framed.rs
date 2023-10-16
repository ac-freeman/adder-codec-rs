use crate::transcoder::source::video::SourceError;
use crate::transcoder::source::video::SourceError::BufferEmpty;
use crate::transcoder::source::video::Video;
use crate::transcoder::source::video::{Source, VideoBuilder};
use adder_codec_core::Mode::FramePerfect;
use adder_codec_core::{DeltaT, Event, PlaneSize, SourceCamera, TimeMode};

use crate::utils::viz::ShowFeatureMode;
use adder_codec_core::codec::{EncoderOptions, EncoderType};
use ndarray::{Array, Axis};
use opencv::core::{Mat, Size};
use opencv::videoio::{VideoCapture, CAP_PROP_FPS, CAP_PROP_FRAME_COUNT, CAP_PROP_POS_FRAMES};
use opencv::{imgproc, prelude::*, videoio, Result};
use rayon::ThreadPool;
use std::io::Write;
use std::mem::swap;
use std::path::PathBuf;
use video_rs::{self, Decoder, Frame, Locator, Options, Resize};

/// Attributes of a framed video -> ADΔER transcode
pub struct Framed<W: Write + 'static> {
    cap: Decoder,
    pub(crate) input_frame: Frame,

    /// Index of the first frame to be read from the input video
    pub frame_idx_start: u32,

    /// FPS of the input video. Set automatically by `Framed::new()`
    pub source_fps: f32,

    /// Scale of the input video. Input frames are resized to this scale before transcoding.
    pub scale: f64,

    /// Whether the input video is color
    color_input: bool,

    pub(crate) video: Video<W>,
}
unsafe impl<W: Write> Sync for Framed<W> {}

impl<W: Write + 'static> Framed<W> {
    /// Create a new `Framed` source
    pub fn new(
        input_filename: String,
        color_input: bool,
        scale: f64,
    ) -> Result<Framed<W>, SourceError> {
        let source = Locator::Path(PathBuf::from(input_filename));
        let mut cap = Decoder::new(&source)?;
        let (width, height) = cap.size();
        cap = Decoder::new_with_options_and_resize(
            &source,
            &Options::default(),
            Resize::Fit(
                ((width as f64) * scale) as u32,
                ((height as f64) * scale) as u32,
            ),
        )?;
        let (width, height) = cap.size();

        // Calculate TPS based on ticks per frame and source FPS
        let source_fps = cap.frame_rate();

        let (time, init_frame) = cap.decode()?;

        let plane = PlaneSize::new(width as u16, height as u16, if color_input { 3 } else { 1 })?;

        let video = Video::new(plane, FramePerfect, None)?;

        Ok(Framed {
            cap,
            input_frame: Frame::default((height as usize, width as usize, 3)), // Note that this will be limited to 8-bit precision (due to video-rs crate)
            frame_idx_start: 0,
            source_fps,
            scale,
            color_input,
            video,
        })
    }

    /// Set the start frame of the source
    pub fn frame_start(mut self, frame_idx_start: u32) -> Result<Self, SourceError> {
        let video_frame_count = self.cap.frame_count();
        if frame_idx_start >= video_frame_count as u32 {
            return Err(SourceError::StartOutOfBounds(frame_idx_start));
        };
        let ts_millis = (frame_idx_start as f32 / self.source_fps * 1000.0) as i64;
        self.cap.reader.seek(ts_millis)?;

        self.frame_idx_start = frame_idx_start;
        Ok(self)
    }

    /// Automatically derive the ticks per second from the source FPS and `ref_time`
    pub fn auto_time_parameters(
        mut self,
        ref_time: DeltaT,
        delta_t_max: DeltaT,
        time_mode: Option<TimeMode>,
    ) -> Result<Self, SourceError> {
        if delta_t_max % ref_time == 0 {
            let tps = (ref_time as f32 * self.source_fps) as DeltaT;
            self.video = self
                .video
                .time_parameters(tps, ref_time, delta_t_max, time_mode)?;
        } else {
            return Err(SourceError::BadParams(
                "delta_t_max must be a multiple of ref_time".to_string(),
            ));
        }
        Ok(self)
    }

    /// Get the number of ticks each frame is said to span
    pub fn get_ref_time(&self) -> u32 {
        self.video.state.ref_time
    }

    pub fn get_last_input_frame(&self) -> &Frame {
        &self.input_frame
    }
}

impl<W: Write + 'static> Source<W> for Framed<W> {
    /// Get pixel-wise intensities directly from source frame, and integrate them with
    /// `ref_time` (the number of ticks each frame is said to span)
    fn consume(
        &mut self,
        view_interval: u32,
        thread_pool: &ThreadPool,
    ) -> Result<Vec<Vec<Event>>, SourceError> {
        let (_, frame) = self.cap.decode()?;
        self.input_frame = handle_color(frame, self.color_input)?;

        thread_pool.install(|| {
            self.video.integrate_matrix(
                self.input_frame.clone(),
                self.video.state.ref_time as f32,
                view_interval,
            )
        })
    }

    fn crf(&mut self, crf: u8) {
        self.video.update_crf(crf, true);
    }

    fn get_video_mut(&mut self) -> &mut Video<W> {
        &mut self.video
    }

    fn get_video_ref(&self) -> &Video<W> {
        &self.video
    }

    fn get_video(self) -> Video<W> {
        todo!()
    }

    fn get_input(&self) -> &Frame {
        self.get_last_input_frame()
    }

    fn get_running_input_bitrate(&self) -> f64 {
        let video = self.get_video_ref();
        video.get_tps() as f64 / video.get_ref_time() as f64
            * video.state.plane.volume() as f64
            * 8.0
    }
}

impl<W: Write + 'static> VideoBuilder<W> for Framed<W> {
    fn contrast_thresholds(mut self, c_thresh_pos: u8, _c_thresh_neg: u8) -> Self {
        self.video = self.video.c_thresh_pos(c_thresh_pos);
        // self.video = self.video.c_thresh_neg(c_thresh_neg);
        self
    }

    fn crf(mut self, crf: u8) -> Self {
        self.video.update_crf(crf, true);
        self
    }

    fn quality_manual(
        mut self,
        c_thresh_baseline: u8,
        c_thresh_max: u8,
        delta_t_max_multiplier: u32,
        c_increase_velocity: u8,
        feature_c_radius_denom: f32,
    ) -> Self {
        self.video.update_quality_manual(
            c_thresh_baseline,
            c_thresh_max,
            delta_t_max_multiplier,
            c_increase_velocity,
            feature_c_radius_denom,
        );
        self
    }

    fn c_thresh_pos(mut self, c_thresh_pos: u8) -> Self {
        self.video = self.video.c_thresh_pos(c_thresh_pos);
        self
    }

    fn c_thresh_neg(mut self, c_thresh_neg: u8) -> Self {
        self.video = self.video.c_thresh_neg(c_thresh_neg);
        self
    }

    fn chunk_rows(mut self, chunk_rows: usize) -> Self {
        self.video = self.video.chunk_rows(chunk_rows);
        self
    }

    fn time_parameters(
        mut self,
        tps: DeltaT,
        ref_time: DeltaT,
        delta_t_max: DeltaT,
        time_mode: Option<TimeMode>,
    ) -> Result<Self, SourceError> {
        if delta_t_max % ref_time == 0 {
            self.video = self
                .video
                .time_parameters(tps, ref_time, delta_t_max, time_mode)?;
        } else {
            eprintln!("delta_t_max must be a multiple of ref_time");
        }
        Ok(self)
    }

    fn write_out(
        mut self,
        source_camera: SourceCamera,
        time_mode: TimeMode,
        encoder_type: EncoderType,
        encoder_options: EncoderOptions,
        write: W,
    ) -> Result<Box<Self>, SourceError> {
        self.video = self.video.write_out(
            Some(source_camera),
            Some(time_mode),
            encoder_type,
            encoder_options,
            write,
        )?;
        Ok(Box::new(self))
    }

    fn show_display(mut self, show_display: bool) -> Self {
        self.video = self.video.show_display(show_display);
        self
    }

    fn detect_features(mut self, detect_features: bool, show_features: ShowFeatureMode) -> Self {
        self.video = self.video.detect_features(detect_features, show_features);
        self
    }
}

fn handle_color(mut input: Frame, color: bool) -> Result<Frame, SourceError> {
    if !color {
        input
            .exact_chunks_mut((1, 1, 3))
            .into_iter()
            .for_each(|mut v| unsafe {
                *v.uget_mut((0, 0, 0)) = (*v.uget((0, 0, 0)) as f64 * 0.114
                    + *v.uget((0, 0, 1)) as f64 * 0.587
                    + *v.uget((0, 0, 2)) as f64 * 0.299)
                    as u8;
                // v = Array::from_elem(
                //     (3),
                //     (v.uget(0) as f64 * 0.114 + v.uget(1) as f64 * 0.587 + v.uget(2) as f64 * 0.299)
                //         as u8,
                // );
            });

        // Map the three color channels to a single grayscale channel
    }
    Ok(input)
}
