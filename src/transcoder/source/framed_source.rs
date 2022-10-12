use crate::transcoder::source::video::Source;
use crate::transcoder::source::video::Video;
use crate::transcoder::source::video::{show_display, SourceError};
use crate::{Codec, Coord, Event, SourceType, D};
use bumpalo::Bump;
use core::default::Default;
use rayon::iter::ParallelIterator;
use rayon::iter::{IndexedParallelIterator, IntoParallelIterator};

use std::mem::swap;

use crate::framer::scale_intensity::FrameValue;
use crate::transcoder::source::video::SourceError::*;
use ndarray::Axis;
use opencv::core::{Mat, Size};
use opencv::videoio::{VideoCapture, CAP_PROP_FPS, CAP_PROP_FRAME_COUNT, CAP_PROP_POS_FRAMES};
use opencv::{imgproc, prelude::*, videoio, Result};

use crate::transcoder::d_controller::DecimationMode;
use crate::transcoder::event_pixel_tree::Mode::{Continuous, FramePerfect};
use crate::transcoder::event_pixel_tree::{DeltaT, Intensity_32};
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
    last_input_frame_scaled: Mat,
    c_thresh_pos: u8,
    c_thresh_neg: u8,

    scale: f64,
    color_input: bool,
    pub(crate) video: Video,
}

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

    pub fn time_parameters(mut self, tps: DeltaT, delta_t_max: DeltaT) -> FramedSourceBuilder {
        self.tps = tps;
        self.delta_t_max = delta_t_max;
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

    pub fn finish(self) -> FramedSource {
        FramedSource::new(self).unwrap()
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

        // Calculate ref time based on TPS and source FPS
        cap.set(CAP_PROP_POS_FRAMES, builder.frame_idx_start as f64)
            .unwrap();
        let source_fps = cap.get(CAP_PROP_FPS).unwrap().round();
        builder.ref_time = (builder.tps as f64 / source_fps) as u32;

        // Handle the edge cases forcefully
        builder.tps = builder.ref_time * cap.get(CAP_PROP_FPS).unwrap().round() as u32;
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
        );

        Ok(FramedSource {
            cap,
            input_frame_scaled: Default::default(),
            input_frame: Default::default(),
            frame_idx_start: builder.frame_idx_start,
            last_input_frame_scaled: Default::default(),
            c_thresh_pos: builder.c_thresh_pos,
            c_thresh_neg: builder.c_thresh_neg,
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
    fn consume(&mut self, view_interval: u32) -> Result<Vec<Vec<Event>>, SourceError> {
        if self.video.in_interval_count == 0 {
            match self.cap.read(&mut self.input_frame) {
                Ok(_) => resize_frame(
                    &self.input_frame,
                    &mut self.input_frame_scaled,
                    self.color_input,
                    self.scale,
                ),
                Err(e) => {
                    panic!("{}", e);
                }
            };

            self.last_input_frame_scaled = self.input_frame_scaled.clone();

            let frame_arr = self.input_frame_scaled.data_bytes().unwrap();

            self.video
                .event_pixel_trees
                .iter_mut()
                .enumerate()
                .for_each(|(idx, px)| {
                    let intensity = frame_arr[idx];
                    let d_start = (intensity as f32).log2().floor() as D;
                    px.arena[0].set_d(d_start);
                    px.base_val = intensity;
                });
        } else {
            match self.cap.read(&mut self.input_frame) {
                Ok(_) => resize_frame(
                    &self.input_frame,
                    &mut self.input_frame_scaled,
                    self.color_input,
                    self.scale,
                ),
                Err(e) => {
                    panic!("{}", e);
                }
            };
        }

        self.video.in_interval_count += 1;
        if self.video.in_interval_count % view_interval == 0 {
            self.video.show_live = true;
        } else {
            self.video.show_live = false;
        }

        if self.input_frame_scaled.empty() {
            eprintln!("End of video");
            return Err(BufferEmpty);
        }

        let frame_arr: &[u8] = self.input_frame_scaled.data_bytes().unwrap();

        let ref_time = self.video.ref_time as f32;
        let px_per_chunk: usize =
            self.video.chunk_rows * self.video.width as usize * self.video.channels as usize;

        // Important: if framing the events simultaneously, then the chunk division must be
        // exactly the same as it is for the framer
        let big_buffer: Vec<Vec<Event>> = self
            .video
            .event_pixel_trees
            .axis_chunks_iter_mut(Axis(0), self.video.chunk_rows)
            .into_par_iter()
            .enumerate()
            .map(|(chunk_idx, mut chunk)| {
                let mut buffer: Vec<Event> = Vec::with_capacity(px_per_chunk);
                let bump = Bump::new();
                let mut base_val = bump.alloc(0);
                let px_idx = bump.alloc(0);
                let frame_val = bump.alloc(0);

                for (chunk_px_idx, px) in chunk.iter_mut().enumerate() {
                    *px_idx = chunk_px_idx + px_per_chunk * chunk_idx;
                    *frame_val = frame_arr[*px_idx];
                    if px.need_to_pop_top {
                        buffer.push(px.pop_top_event(Some(*frame_val as Intensity_32)));
                    }

                    base_val = &mut px.base_val;

                    if *frame_val < base_val.saturating_sub(self.c_thresh_neg)
                        || *frame_val > base_val.saturating_add(self.c_thresh_pos)
                    {
                        px.pop_best_events(Some(*frame_val as Intensity_32), &mut buffer);
                        px.base_val = *frame_val;
                    }

                    px.integrate(
                        *frame_val as Intensity_32,
                        ref_time,
                        &Continuous,
                        &self.video.delta_t_max,
                    );
                }
                buffer
            })
            .collect();

        if self.video.write_out {
            self.video.stream.encode_events_events(&big_buffer);
        }

        show_display("Gray input", &self.input_frame_scaled, 1, &self.video);
        // self.video.instantaneous_display_frame = (self.input_frame_scaled).clone();

        // for r in 0..self.video.height as i32 {
        //     for c in 0..self.video.width as i32 {
        //         let inst_px: &mut u8 = self.video.instantaneous_frame.at_2d_mut(r, c).unwrap();
        //         let px = &mut self.video.event_pixel_trees[[r as usize, c as usize, 0]];
        //         *inst_px = match px.arena[0].best_event.clone() {
        //             Some(event) => u8::get_frame_value(&event, SourceType::U8, ref_time as DeltaT),
        //             None => 0,
        //         };
        //     }
        // }
        // show_display("instance", &self.video.instantaneous_frame, 1, &self.video);

        Ok(big_buffer)
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

fn resize_frame(input: &Mat, output: &mut Mat, color: bool, scale: f64) {
    let mut holder = Mat::default();
    if !color {
        // Yields an 8-bit grayscale mat
        match imgproc::cvt_color(&input, &mut holder, imgproc::COLOR_BGR2GRAY, 1) {
            Ok(_) => {}
            Err(_) => {
                // don't do anything with the error. This happens when we reach the end of
                // the video, so there's nothing to convert.
            }
        }
    } else {
        holder = input.clone();
    }

    match resize_input(&mut holder, output, scale) {
        Ok(_) => {}
        Err(_) => {}
    };
}
