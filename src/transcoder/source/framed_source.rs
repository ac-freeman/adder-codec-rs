use crate::transcoder::source::video::Source;
use crate::transcoder::source::video::SourceError;
use crate::transcoder::source::video::Video;
use crate::{Coord, Event};

use core::default::Default;

use std::mem::swap;

use crate::transcoder::source::video::SourceError::*;

use opencv::core::{Mat, Size};
use opencv::videoio::{VideoCapture, CAP_PROP_FPS, CAP_PROP_FRAME_COUNT, CAP_PROP_POS_FRAMES};
use opencv::{imgproc, prelude::*, videoio, Result};
use rayon::ThreadPool;

use crate::transcoder::d_controller::DecimationMode;
use crate::transcoder::event_pixel_tree::DeltaT;
use crate::transcoder::event_pixel_tree::Mode::FramePerfect;
use crate::SourceCamera;

#[derive(Debug, Copy, Clone)]
#[repr(C)]
pub struct IndirectCoord {
    pub(crate) forward: Coord,
    pub(crate) reverse: Coord,
}

/// Attributes of a framed video -> ADΔER transcode
pub struct FramedSource {
    cap: VideoCapture,
    pub(crate) input_frame_scaled: Mat,
    pub(crate) input_frame: Mat,
    pub frame_idx_start: u32,
    pub source_fps: f64,
    pub scale: f64,
    color_input: bool,
    pub(crate) video: Video,
}
unsafe impl Sync for FramedSource {}

pub struct FramedSourceBuilder {
    input_filename: String,
    output_events_filename: Option<String>,
    frame_idx_start: u32,
    chunk_rows: usize,
    ref_time: DeltaT,
    tps: DeltaT,
    delta_t_max: DeltaT,
    scale: f64,
    frame_skip_interval: u8,
    color_input: bool,
    c_thresh_pos: u8,
    c_thresh_neg: u8,
    write_out: bool,
    communicate_events: bool,
    show_display_b: bool,
    source_camera: SourceCamera,
}

impl FramedSourceBuilder {
    pub fn new(input_filename: String, source_camera: SourceCamera) -> FramedSourceBuilder {
        FramedSourceBuilder {
            input_filename,
            output_events_filename: None,
            frame_idx_start: 0,
            chunk_rows: 0,
            ref_time: 5000,
            tps: 150000,
            delta_t_max: 150000,
            scale: 1.0,
            frame_skip_interval: 0,
            color_input: true,
            c_thresh_pos: 0,
            c_thresh_neg: 0,
            write_out: false,
            communicate_events: false,
            show_display_b: false,
            source_camera,
        }
    }

    pub fn output_events_filename(mut self, output_events_filename: String) -> FramedSourceBuilder {
        self.output_events_filename = Some(output_events_filename);
        self.write_out = true;
        self
    }

    pub fn frame_start(mut self, frame_idx_start: u32) -> FramedSourceBuilder {
        self.frame_idx_start = frame_idx_start;
        self
    }

    pub fn chunk_rows(mut self, chunk_rows: usize) -> FramedSourceBuilder {
        self.chunk_rows = chunk_rows;
        self
    }

    pub fn time_parameters(mut self, ref_time: DeltaT, delta_t_max: DeltaT) -> FramedSourceBuilder {
        self.delta_t_max = delta_t_max;
        self.ref_time = ref_time;
        assert_eq!(self.delta_t_max % self.ref_time, 0);
        self
    }

    pub fn contrast_thresholds(
        mut self,
        c_thresh_pos: u8,
        c_thresh_neg: u8,
    ) -> FramedSourceBuilder {
        self.c_thresh_pos = c_thresh_pos;
        self.c_thresh_neg = c_thresh_neg;
        self
    }

    pub fn scale(mut self, scale: f64) -> FramedSourceBuilder {
        self.scale = scale;
        self
    }

    pub fn skip_interval(mut self, frame_skip_interval: u8) -> FramedSourceBuilder {
        self.frame_skip_interval = frame_skip_interval;
        self
    }

    pub fn color(mut self, color_input: bool) -> FramedSourceBuilder {
        self.color_input = color_input;
        self
    }

    pub fn communicate_events(mut self, communicate_events: bool) -> FramedSourceBuilder {
        self.communicate_events = communicate_events;
        self
    }

    pub fn show_display(mut self, show_display_b: bool) -> FramedSourceBuilder {
        self.show_display_b = show_display_b;
        self
    }

    pub fn finish(self) -> Result<FramedSource> {
        FramedSource::new(self)
    }
}

impl FramedSource {
    /// Initialize the framed source and read first frame of source, in order to get `height`
    /// and `width` and initialize [`Video`]
    fn new(mut builder: FramedSourceBuilder) -> Result<FramedSource> {
        let channels = match builder.color_input {
            true => 3,
            false => 1,
        };

        let mut cap =
            videoio::VideoCapture::from_file(builder.input_filename.as_str(), videoio::CAP_FFMPEG)?;
        let video_frame_count = cap.get(CAP_PROP_FRAME_COUNT).unwrap();
        assert!(builder.frame_idx_start < video_frame_count as u32);

        // Calculate TPS based on ticks per frame and source FPS
        cap.set(CAP_PROP_POS_FRAMES, builder.frame_idx_start as f64)
            .unwrap();
        let source_fps = cap.get(CAP_PROP_FPS).unwrap().round();
        builder.tps = builder.ref_time * source_fps as u32;
        assert_eq!(
            builder.ref_time * cap.get(CAP_PROP_FPS).unwrap().round() as u32,
            builder.tps
        );

        let opened = videoio::VideoCapture::is_opened(&cap)?;
        if !opened {
            panic!("Could not open source")
        }
        let mut init_frame = Mat::default();
        match cap.read(&mut init_frame) {
            Ok(_) => {}
            Err(e) => {
                panic!("{}", e);
            }
        };

        // Move start frame back
        cap.set(CAP_PROP_POS_FRAMES, builder.frame_idx_start as f64)
            .unwrap();

        let mut init_frame_scaled = Mat::default();
        println!("Original width is {}", init_frame.size()?.width);
        resize_input(&mut init_frame, &mut init_frame_scaled, builder.scale).unwrap();

        init_frame = init_frame_scaled;

        println!("Width is {}", init_frame.size()?.width);

        // Sanity checks
        // assert!(init_frame.size()?.width > 50);
        // assert!(init_frame.size()?.height > 50);

        let video = Video::new(
            init_frame.size()?.width as u16,
            init_frame.size()?.height as u16,
            builder.chunk_rows,
            builder.output_events_filename,
            channels,
            builder.tps,
            builder.ref_time,
            builder.delta_t_max,
            DecimationMode::Manual,
            builder.write_out,
            builder.communicate_events,
            builder.show_display_b,
            builder.source_camera,
            builder.c_thresh_pos,
            builder.c_thresh_neg,
        );

        Ok(FramedSource {
            cap,
            input_frame_scaled: Default::default(),
            input_frame: Default::default(),
            frame_idx_start: builder.frame_idx_start,
            source_fps,
            scale: builder.scale,
            color_input: builder.color_input,
            video,
        })
    }

    pub fn get_ref_time(&self) -> u32 {
        self.video.ref_time
    }
}

impl Source for FramedSource {
    /// Get pixel-wise intensities directly from source frame, and integrate them with
    /// [`ref_time`](Video::ref_time) (the number of ticks each frame is said to span)
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
            Err(e) => {
                panic!("{}", e);
            }
        };

        if self.input_frame_scaled.empty() {
            eprintln!("End of video");
            return Err(BufferEmpty);
        }

        let tmp = self.input_frame_scaled.clone();
        thread_pool.install(|| {
            self.video.integrate_matrix(
                tmp,
                self.video.ref_time as f32,
                FramePerfect,
                view_interval,
            )
        })
    }

    fn get_video_mut(&mut self) -> &mut Video {
        &mut self.video
    }

    fn get_video(&self) -> &Video {
        &self.video
    }
}

/// Resize a grayscale [`Mat`]
fn resize_input(
    input_frame_gray: &mut Mat,
    input_frame_scaled: &mut Mat,
    resize_scale: f64,
) -> Result<(), opencv::Error> {
    if resize_scale != 1.0 {
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
    } else {
        // For performance. We don't need to read input_frame_gray again anyway
        swap(input_frame_gray, input_frame_scaled);
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
    if !color {
        // Yields an 8-bit grayscale mat
        imgproc::cvt_color(&input, &mut holder, imgproc::COLOR_BGR2GRAY, 1)?;
        // don't do anything with the error. This happens when we reach the end of
        // the video, so there's nothing to convert.
    } else {
        holder = input.clone();
    }

    resize_input(&mut holder, output, scale)?;
    Ok(())
}
