use crate::transcoder::source::video::SourceError;
use crate::transcoder::source::video::SourceError::BufferEmpty;
use crate::transcoder::source::video::Video;
use crate::transcoder::source::video::{Source, VideoBuilder};
use adder_codec_core::Mode::FramePerfect;
use adder_codec_core::{DeltaT, Event, PlaneSize, SourceCamera, TimeMode};

use adder_codec_core::codec::EncoderOptions;
use opencv::core::{Mat, Size};
use opencv::videoio::{VideoCapture, CAP_PROP_FPS, CAP_PROP_FRAME_COUNT, CAP_PROP_POS_FRAMES};
use opencv::{imgproc, prelude::*, videoio, Result};
use rayon::ThreadPool;
use std::io::Write;
use std::mem::swap;

/// Attributes of a framed video -> ADΔER transcode
pub struct Framed<W: Write + 'static> {
    cap: VideoCapture,
    pub(crate) input_frame_scaled: Mat,
    pub(crate) input_frame: Mat,

    /// Index of the first frame to be read from the input video
    pub frame_idx_start: u32,

    /// FPS of the input video. Set automatically by `Framed::new()`
    pub source_fps: f64,

    /// Scale of the input video. Input frames are resized to this scale before transcoding.
    pub scale: f64,

    /// Whether the input video is color
    color_input: bool,

    pub(crate) video: Video<W>,

    /// Time mode of the source
    pub time_mode: TimeMode,
}
unsafe impl<W: Write> Sync for Framed<W> {}

impl<W: Write + 'static> Framed<W> {
    /// Create a new `Framed` source
    pub fn new(
        input_filename: String,
        color_input: bool,
        scale: f64,
    ) -> Result<Framed<W>, SourceError> {
        let mut cap =
            videoio::VideoCapture::from_file(input_filename.as_str(), videoio::CAP_FFMPEG)?;

        // Calculate TPS based on ticks per frame and source FPS
        let source_fps = cap.get(CAP_PROP_FPS)?.round();
        // builder.tps = builder.ref_time * source_fps as u32;
        // if builder.ref_time * cap.get(CAP_PROP_FPS)?.round() as u32 != builder.tps {
        //     return Err(SourceError::BadParams.into());
        // }

        let opened = videoio::VideoCapture::is_opened(&cap)?;
        if !opened {
            return Err(SourceError::Open);
        }
        let mut init_frame = Mat::default();
        cap.read(&mut init_frame)?;
        cap.set(CAP_PROP_POS_FRAMES, 0.0)?;

        // Move start frame back
        // cap.set(CAP_PROP_POS_FRAMES, f64::from(builder.frame_idx_start))?;

        let mut init_frame_scaled = Mat::default();
        resize_input(&mut init_frame, &mut init_frame_scaled, scale)?;
        init_frame = init_frame_scaled;

        let plane = PlaneSize::new(
            init_frame.size()?.width as u16,
            init_frame.size()?.height as u16,
            if color_input { 3 } else { 1 },
        )?;

        let video = Video::new(plane, FramePerfect, None)?;

        Ok(Framed {
            cap,
            input_frame_scaled: Mat::default(),
            input_frame: Mat::default(),
            frame_idx_start: 0,
            source_fps,
            scale,
            color_input,
            video,
            time_mode: TimeMode::default(),
        })
    }

    /// Set the start frame of the source
    pub fn frame_start(mut self, frame_idx_start: u32) -> Result<Self, SourceError> {
        let video_frame_count = self.cap.get(CAP_PROP_FRAME_COUNT)?;
        if frame_idx_start >= video_frame_count as u32 {
            return Err(SourceError::StartOutOfBounds(frame_idx_start));
        };
        self.cap
            .set(CAP_PROP_POS_FRAMES, f64::from(frame_idx_start))?;
        self.frame_idx_start = frame_idx_start;
        Ok(self)
    }

    /// Set the [`TimeMode`](adder_codec_core::TimeMode) for the source
    pub fn time_mode(mut self, time_mode: TimeMode) -> Self {
        self.time_mode = time_mode;
        self
    }

    /// Automatically derive the ticks per second from the source FPS and `ref_time`
    pub fn auto_time_parameters(
        mut self,
        ref_time: DeltaT,
        delta_t_max: DeltaT,
        time_mode: Option<TimeMode>,
    ) -> Result<Self, SourceError> {
        if delta_t_max % ref_time == 0 {
            let tps = (ref_time as f64 * self.source_fps) as DeltaT;
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
}

impl<W: Write + 'static> Source<W> for Framed<W> {
    /// Get pixel-wise intensities directly from source frame, and integrate them with
    /// `ref_time` (the number of ticks each frame is said to span)
    fn consume(
        &mut self,
        view_interval: u32,
        thread_pool: &ThreadPool,
    ) -> Result<Vec<Vec<Event>>, SourceError> {
        match self.cap.read(&mut self.input_frame) {
            Ok(_) => {
                match resize_frame(
                    &self.input_frame,
                    &mut self.input_frame_scaled,
                    self.color_input,
                    self.scale,
                ) {
                    Ok(_) => {}
                    Err(_) => return Err(SourceError::NoData),
                }
            }
            Err(e) => return Err(SourceError::OpencvError(e)),
        };

        if self.input_frame_scaled.empty() {
            return Err(BufferEmpty);
        }

        let tmp = self.input_frame_scaled.clone();

        thread_pool.install(|| {
            self.video
                .integrate_matrix(tmp, self.video.state.ref_time as f32, view_interval)
        })
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
}

impl<W: Write + 'static> VideoBuilder<W> for Framed<W> {
    fn contrast_thresholds(mut self, c_thresh_pos: u8, c_thresh_neg: u8) -> Self {
        self.video = self.video.c_thresh_pos(c_thresh_pos);
        self.video = self.video.c_thresh_neg(c_thresh_neg);
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
        encoder_options: EncoderOptions,
        write: W,
    ) -> Result<Box<Self>, SourceError> {
        self.video =
            self.video
                .write_out(Some(source_camera), Some(time_mode), encoder_options, write)?;
        Ok(Box::new(self))
    }

    fn show_display(mut self, show_display: bool) -> Self {
        self.video = self.video.show_display(show_display);
        self
    }
}

/// Resize a grayscale [`Mat`]
fn resize_input(
    input_frame_gray: &mut Mat,
    input_frame_scaled: &mut Mat,
    resize_scale: f64,
) -> Result<(), opencv::Error> {
    if (resize_scale - 1.0).abs() < f64::EPSILON {
        // For performance. We don't need to read input_frame_gray again anyway
        swap(input_frame_gray, input_frame_scaled);
    } else {
        opencv::imgproc::resize(
            input_frame_gray,
            input_frame_scaled,
            Size {
                width: 0,
                height: 0,
            },
            resize_scale,
            resize_scale,
            0,
        )?;
    }
    Ok(())
}

fn resize_frame(
    input: &Mat,
    output: &mut Mat,
    color: bool,
    scale: f64,
) -> Result<(), opencv::Error> {
    let mut holder = Mat::default();
    if color {
        holder = input.clone();
    } else {
        // Yields an 8-bit grayscale mat
        imgproc::cvt_color(&input, &mut holder, imgproc::COLOR_BGR2GRAY, 1)?;
        // don't do anything with the error. This happens when we reach the end of
        // the video, so there's nothing to convert.
    }

    resize_input(&mut holder, output, scale)?;
    Ok(())
}
