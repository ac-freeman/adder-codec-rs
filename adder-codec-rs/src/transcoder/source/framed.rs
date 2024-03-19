use crate::transcoder::source::video::SourceError;
use crate::transcoder::source::video::Video;
use crate::transcoder::source::video::{Source, VideoBuilder};
use adder_codec_core::Mode::FramePerfect;
use adder_codec_core::{DeltaT, Event, PixelMultiMode, PlaneSize, SourceCamera, TimeMode};

use crate::utils::viz::ShowFeatureMode;
use adder_codec_core::codec::{EncoderOptions, EncoderType};

use crate::utils::cv::handle_color;
#[cfg(feature = "feature-logging")]
use crate::utils::cv::{calculate_quality_metrics, QualityMetrics};

use rayon::ThreadPool;
use std::io::Write;
use std::path::PathBuf;

#[cfg(feature = "feature-logging")]
use chrono::Local;
use tokio::runtime::Runtime;
use video_rs_adder_dep::{self, Decoder, Frame, Locator, Options, Resize};

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
        input_path: PathBuf,
        color_input: bool,
        scale: f64,
    ) -> Result<Framed<W>, SourceError> {
        let source = Locator::Path(input_path);
        let mut cap = Decoder::new(&source)?;
        let (width, height) = cap.size();
        let width = ((width as f64) * scale) as u32;
        let height = ((height as f64) * scale) as u32;

        cap = Decoder::new_with_options_and_resize(
            &source,
            &Options::default(),
            Resize::Fit(width, height),
        )?;

        // Calculate TPS based on ticks per frame and source FPS
        let source_fps = cap.frame_rate();
        let (width, height) = cap.size_out();

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
        self.video.state.params.ref_time
    }

    /// Get the previous input frame
    pub fn get_last_input_frame(&self) -> &Frame {
        &self.input_frame
    }
}

impl<W: Write + 'static> Source<W> for Framed<W> {
    /// Get pixel-wise intensities directly from source frame, and integrate them with
    /// `ref_time` (the number of ticks each frame is said to span)
    fn consume(&mut self, thread_pool: &Runtime) -> Result<Vec<Vec<Event>>, SourceError> {
        let (_, frame) = self.cap.decode()?;
        self.input_frame = handle_color(frame, self.color_input)?;

        let res = self.video.integrate_matrix(
            self.input_frame.clone(),
            self.video.state.params.ref_time as f32,
        );
        #[cfg(feature = "feature-logging")]
        {
            if let Some(handle) = &mut self.video.state.feature_log_handle {
                // Calculate the quality metrics
                let mut image_mat = self.video.state.running_intensities.clone();

                #[rustfmt::skip]
                    let metrics = calculate_quality_metrics(
                    &self.input_frame,
                    &image_mat,
                    QualityMetrics {
                        mse: Some(0.0),
                        psnr: Some(0.0),
                        ssim: None,
                    });

                let metrics = metrics.unwrap();
                let bytes = serde_pickle::to_vec(&metrics, Default::default()).unwrap();
                handle.write_all(&bytes).unwrap();
            }
        }
        res
    }

    fn crf(&mut self, crf: u8) {
        self.video.update_crf(crf);
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

    fn get_input(&self) -> Option<&Frame> {
        Some(self.get_last_input_frame())
    }

    fn get_running_input_bitrate(&self) -> f64 {
        let video = self.get_video_ref();
        video.get_tps() as f64 / video.get_ref_time() as f64
            * video.state.plane.volume() as f64
            * 8.0
    }
}

impl<W: Write + 'static> VideoBuilder<W> for Framed<W> {
    fn crf(mut self, crf: u8) -> Self {
        self.video.update_crf(crf);
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
        pixel_multi_mode: PixelMultiMode,
        adu_interval: Option<usize>,
        encoder_type: EncoderType,
        encoder_options: EncoderOptions,
        write: W,
    ) -> Result<Box<Self>, SourceError> {
        self.video = self.video.write_out(
            Some(source_camera),
            Some(time_mode),
            Some(pixel_multi_mode),
            adu_interval,
            encoder_type,
            encoder_options,
            write,
        )?;
        Ok(Box::new(self))
    }

    fn detect_features(mut self, detect_features: bool, show_features: ShowFeatureMode) -> Self {
        self.video = self.video.detect_features(detect_features, show_features);
        self
    }

    #[cfg(feature = "feature-logging")]
    fn log_path(mut self, name: String) -> Self {
        let date_time = Local::now();
        let formatted = format!("{}_{}.log", name, date_time.format("%d_%m_%Y_%H_%M_%S"));
        let log_handle = std::fs::File::create(formatted).ok();
        self.video.state.feature_log_handle = log_handle;

        // Write the plane size to the log file
        if let Some(handle) = &mut self.video.state.feature_log_handle {
            writeln!(
                handle,
                "{}x{}x{}",
                self.video.state.plane.w(),
                self.video.state.plane.h(),
                self.video.state.plane.c()
            )
            .unwrap();
        }
        self
    }
}
