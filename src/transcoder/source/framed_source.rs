use crate::transcoder::event_pixel::{DeltaT, Intensity};
use crate::transcoder::source::video::show_display;
use crate::transcoder::source::video::Source;
use crate::transcoder::source::video::Video;
use crate::{Codec, Coord, Event, D, D_MAX};
use core::default::Default;
use std::cmp::max;
use std::collections::VecDeque;
use std::mem::swap;
use std::sync::mpsc::{channel, Receiver, Sender};

use ndarray::Axis;
use opencv::core::{Mat, Size};
use opencv::videoio::{VideoCapture, CAP_PROP_FPS, CAP_PROP_FRAME_COUNT, CAP_PROP_POS_FRAMES};
use opencv::{imgproc, prelude::*, videoio, Result};

use crate::transcoder::event_pixel::pixel::Transition;
use crate::SourceCamera;

#[derive(Debug, Copy, Clone)]
#[repr(C)]
pub struct IndirectCoord {
    pub(crate) forward: Coord,
    pub(crate) reverse: Coord,
}

/// Attributes of a framed video -> ADΔER transcode
pub struct FramedSource {
    // cap: VideoCapture,
    // frame_buffer: FrameBuffer,
    buffer_tx: Sender<i32>,
    frame_rx: Receiver<Box<Mat>>,
    pub(crate) input_frame_scaled: Box<Mat>,
    last_input_frame_scaled: Box<Mat>,
    c_thresh_pos: u8,
    c_thresh_neg: u8,

    /// Only used when [`look_ahead`](MyArgs::look_ahead) is `true`
    lookahead_frames_scaled: VecDeque<Box<Mat>>,
    pub(crate) video: Video,
}

pub struct FramedSourceBuilder {
    input_filename: String,
    output_events_filename: Option<String>,
    frame_idx_start: u32,
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

    pub fn time_parameters(
        mut self,
        ref_time: DeltaT,
        tps: DeltaT,
        delta_t_max: DeltaT,
    ) -> FramedSourceBuilder {
        self.ref_time = ref_time;
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
    fn new(builder: FramedSourceBuilder) -> Result<FramedSource> {
        let channels = match builder.color_input {
            true => 3,
            false => 1,
        };

        let mut cap =
            videoio::VideoCapture::from_file(builder.input_filename.as_str(), videoio::CAP_FFMPEG)?;
        let video_frame_count = cap.get(CAP_PROP_FRAME_COUNT).unwrap();
        assert!(builder.frame_idx_start < video_frame_count as u32);

        cap.set(CAP_PROP_POS_FRAMES, builder.frame_idx_start as f64)
            .unwrap();

        let mut cap_lookahead =
            videoio::VideoCapture::from_file(builder.input_filename.as_str(), videoio::CAP_FFMPEG)
                .unwrap();
        init_lookahead(builder.frame_idx_start, true, &mut cap_lookahead);
        assert_eq!(
            builder.ref_time * cap.get(CAP_PROP_FPS).unwrap().round() as u32,
            builder.tps
        );

        let opened = videoio::VideoCapture::is_opened(&cap)?;
        if !opened {
            panic!("Unable to open video");
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
            builder.output_events_filename,
            channels,
            builder.tps,
            builder.ref_time,
            builder.delta_t_max,
            0,
            builder.write_out,
            builder.communicate_events,
            builder.show_display_b,
            builder.source_camera,
        );

        let mut frame_buffer: FrameBuffer = FrameBuffer::new(
            (builder.delta_t_max / builder.ref_time) as usize + 1,
            cap,
            builder.frame_skip_interval,
            builder.scale,
            builder.color_input,
        );
        let (buffer_tx, buffer_rx): (Sender<i32>, Receiver<i32>) = channel();
        let (frame_tx, frame_rx): (Sender<Box<Mat>>, Receiver<Box<Mat>>) = channel();

        // Spin off a thread for managing the input frame buffer. It will keep the buffer filled,
        // and pre-process the next input frame (grayscale conversion and rescaling)
        rayon::spawn(move || loop {
            match buffer_rx.recv() {
                Ok(_) => {
                    match frame_buffer.ensure_one_frame() {
                        true => {}
                        false => {
                            eprintln!("Reached end of video file. Exiting channel.");
                            break;
                        }
                    };
                    match frame_buffer.pop_frame() {
                        None => {
                            eprintln!("Video is over. Exiting channel.");
                            break;
                        }
                        Some(frame) => {
                            match frame_tx.send(frame) {
                                Ok(_) => {}
                                Err(_) => {
                                    eprintln!(
                                        "Frame buffer transmitter is closed. Exiting channel."
                                    );
                                    break;
                                }
                            };
                        }
                    }
                    frame_buffer.prep_frame();
                    if frame_buffer.input_frame_queue.is_empty() {
                        println!("END OF FRAME BUFFER");
                        break;
                    }
                }
                Err(_) => {
                    eprintln!("Frame buffer receiver is closed. Exiting channel.");
                    break;
                }
            };
        });
        buffer_tx.send(1).unwrap();

        Ok(FramedSource {
            // cap,
            // frame_buffer,
            buffer_tx,
            frame_rx,
            input_frame_scaled: Default::default(),
            last_input_frame_scaled: Default::default(),
            c_thresh_pos: builder.c_thresh_pos,
            c_thresh_neg: builder.c_thresh_neg,
            lookahead_frames_scaled: Default::default(),
            video,
        })
    }
}

impl Source for FramedSource {
    /// Get pixel-wise intensities directly from source frame, and integrate them with
    /// [`ref_time`](Video::ref_time) (the number of ticks each frame is said to span)
    fn consume(&mut self, view_interval: u32) -> Result<Vec<Vec<Event>>, &'static str> {
        if self.video.in_interval_count == 0 {
            self.input_frame_scaled = match self.frame_rx.recv() {
                Err(_) => return Err("End of video"), // TODO: make it a proper rust error
                Ok(a) => a,
            };
            self.buffer_tx.send(1).unwrap();

            let frame_arr = self.input_frame_scaled.data_bytes().unwrap();

            self.video
                .event_pixels
                .iter_mut()
                .enumerate()
                .for_each(|(idx, px)| {
                    let intensity = frame_arr[idx];
                    let d_start = (intensity as f32).log2().floor() as D;
                    px.d_controller.set_d(d_start);
                });
        } else {
            swap(
                &mut self.last_input_frame_scaled,
                &mut self.input_frame_scaled,
            );
            self.input_frame_scaled = self.lookahead_frames_scaled.pop_front().unwrap();
        }

        self.video.in_interval_count += 1;
        if self.video.in_interval_count % view_interval == 0 {
            self.video.show_live = true;
        } else {
            self.video.show_live = false;
        }

        while self.lookahead_frames_scaled.len()
            < (self.video.delta_t_max / self.video.ref_time) as usize - 1
        {
            self.lookahead_frames_scaled
                .push_back(match self.frame_rx.recv() {
                    // Haning when there's no message left
                    Err(_) => return Err("End of video"), // TODO: make it a proper rust error
                    Ok(a) => a,
                });
            match self.buffer_tx.send(1) {
                Ok(_) => {}
                Err(_e) => {
                    // eprintln!("{}", e)
                }
            };
        }

        match self.buffer_tx.send(1) {
            Ok(_) => {}
            Err(_e) => {
                // eprintln!("{}", e)
            }
        };

        if (*self.input_frame_scaled).empty() || (self.lookahead_frames_scaled[0].empty()) {
            eprintln!("End of video");
            return Err("End of video");
        }

        self.video.input_frame_8u = (*self.input_frame_scaled).clone();

        let frame_arr: &[u8] = self.input_frame_scaled.data_bytes().unwrap();

        let mut data_bytes: Vec<&[u8]> = Vec::new();
        for i in 0..self.lookahead_frames_scaled.len() {
            match (*self.lookahead_frames_scaled[i]).data_bytes() {
                Ok(bytes) => {
                    data_bytes.push(bytes);
                }
                _ => {
                    return Err("End of video");
                }
            }
        }

        let dtm = self.video.delta_t_max;
        let ref_time = self.video.ref_time as f32;

        let chunk_rows = self.video.height as usize / rayon::current_num_threads() as usize;
        let px_per_chunk: usize =
            chunk_rows * self.video.width as usize * self.video.channels as usize;
        let big_buffer: Vec<_> = self
            .video
            .event_pixels
            .axis_chunks_iter_mut(Axis(0), chunk_rows)
            // .into_par_iter()
            .enumerate()
            .map(|(chunk_idx, mut chunk)| {
                let mut buffer: Vec<Event> = Vec::with_capacity(100);
                for (chunk_px_idx, px) in chunk.iter_mut().enumerate() {
                    let px_idx = chunk_px_idx + px_per_chunk * chunk_idx;

                    px.reset_fire_count();

                    if self.video.in_interval_count == px.next_transition.frame_idx {
                        // c_val is the pixel's value on the input frame we're integrating
                        let c_val: u8 = frame_arr[px_idx];

                        px.lookahead_reset(&mut buffer);

                        let mut i = 0;
                        let mut next_val: u8;
                        let mut intensity_sum = c_val as f32;
                        let mut current_d = (intensity_sum).log2().floor() as D;
                        let mut ideal_i = 0;

                        // data_bytes stores the lookahead pixel values
                        while i < data_bytes.len() {
                            next_val = data_bytes[i][px_idx];

                            if next_val >= c_val.saturating_sub(self.c_thresh_neg)
                                && next_val <= c_val.saturating_add(self.c_thresh_pos)
                            {
                                i += 1;
                                intensity_sum += next_val as f32;
                                if (intensity_sum).log2().floor() as D > current_d
                                    || (intensity_sum == 0.0)
                                {
                                    current_d = (intensity_sum).log2().floor() as D;
                                    ideal_i = i;
                                }
                            } else {
                                break;
                            }
                        }

                        let trans = match ideal_i {
                            0 => Transition {
                                frame_idx: self.video.in_interval_count + 1,
                            },
                            _ => Transition {
                                frame_idx: self.video.in_interval_count + ideal_i as u32 + 1,
                            },
                        };
                        px.next_transition = trans;
                        let d_to_set = (intensity_sum).log2().floor() as D;
                        px.d_controller.set_d(current_d);
                        assert!(d_to_set <= D_MAX);
                    }

                    let intensity = frame_arr[px_idx] as Intensity;
                    px.add_intensity(
                        intensity,
                        ref_time,
                        &dtm,
                        &mut buffer,
                        self.video.communicate_events,
                    );

                    px.last_event.calc_frame_intensity(ref_time as u32);
                    px.last_event.calc_frame_delta_t(dtm);
                }
                buffer
            })
            .collect();
        if self.video.write_out {
            self.video.stream.encode_events_events(&big_buffer);
        }

        show_display("Gray input", &self.input_frame_scaled, 1, &self.video);
        self.video.instantaneous_display_frame = (*self.input_frame_scaled).clone();
        Ok(big_buffer)
    }

    fn get_video_mut(&mut self) -> &mut Video {
        &mut self.video
    }

    fn get_video(&self) -> &Video {
        &self.video
    }
}

/// Initialize optional lookahead attributes
fn init_lookahead(frame_idx_start: u32, lookahead: bool, cap_lookahead: &mut VideoCapture) {
    if lookahead {
        // let lookahead_distance = args.delta_t_max / args.ref_time;
        // let lookahead_distance = cap_lookahead.get(CAP_PROP_FPS).unwrap();
        // let lookahead_distance = cap_lookahead.get(CAP_PROP_FPS).unwrap(); // This allows the ROI stuff to still work properly
        let lookahead_distance = 1;
        println!(
            "Source FPS is {}. Looking ahead by {} frames",
            cap_lookahead.get(CAP_PROP_FPS).unwrap(),
            lookahead_distance
        );
        cap_lookahead
            .set(
                CAP_PROP_POS_FRAMES,
                (lookahead_distance + frame_idx_start) as f64,
            )
            .unwrap();
    }
}

/// Resize a grayscale [`Mat`]
fn resize_input(
    input_frame_gray: &mut Mat,
    input_frame_scaled: &mut Mat,
    resize_scale: f64,
) -> Result<(), opencv::Error> {
    // *input_frame_gray = input_frame_gray.col_range(&Range::new(2450 as i32, 2600 as i32).unwrap()).unwrap();
    // *input_frame_gray = input_frame_gray.row_range(&Range::new(1500 as i32, 1530 as i32).unwrap()).unwrap();
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
        std::mem::swap(input_frame_gray, input_frame_scaled);
    }
    Ok(())
}

struct FrameBuffer {
    input_frame_queue: VecDeque<Box<Mat>>,
    cap: VideoCapture,
    frame_skip_interval: u8,
    buffer_size: usize,
    frame_holder: Mat,
    frame_gray_holder: Mat,
    resize_scale: f64,
    color_input: bool,
}

impl FrameBuffer {
    pub fn new(
        buffer_size: usize,
        cap: VideoCapture,
        frame_skip_interval: u8,
        resize_scale: f64,
        color_input: bool,
    ) -> FrameBuffer {
        let input_frame_queue: VecDeque<Box<Mat>> = VecDeque::with_capacity(60);

        FrameBuffer {
            input_frame_queue,
            cap,
            frame_skip_interval,
            buffer_size,
            frame_holder: Mat::default(),
            frame_gray_holder: Mat::default(),
            resize_scale,
            color_input,
        }
    }

    fn fill_buffer(&mut self) {
        // Fill the buffer

        while self.input_frame_queue.len() < self.buffer_size {
            let current_len = self.input_frame_queue.len();
            match self.get_next_image() {
                false => break,
                _ => {
                    if self.input_frame_queue.len() == current_len {
                        break; // Then we're at the end of the video and have emptied the buffer
                    }
                }
            };
        }
    }

    fn get_next_image(&mut self) -> bool {
        for _ in 0..self.frame_skip_interval {
            // Grab (but don't decode or process at all) the frames we're going to to ignore
            match self.cap.grab() {
                Ok(_) => {}
                Err(e) => {
                    eprintln!("{}, could not grab", e)
                }
            };
        }
        match self.cap.read(&mut self.frame_holder) {
            Ok(_) => {}
            Err(e) => {
                eprintln!("{}, could not read", e)
            }
        };

        if !self.color_input {
            // Yields an 8-bit grayscale mat
            match imgproc::cvt_color(
                &self.frame_holder,
                &mut self.frame_gray_holder,
                imgproc::COLOR_BGR2GRAY,
                1,
            ) {
                Ok(_) => {}
                Err(_) => {
                    // don't do anything with the error. This happens when we reach the end of
                    // the video, so there's nothing to convert.
                    return true;
                }
            }
        } else {
            self.frame_gray_holder = self.frame_holder.clone();
        }

        match resize_input(
            &mut self.frame_gray_holder,
            &mut self.frame_holder,
            self.resize_scale,
        ) {
            Ok(_) => {}
            Err(_) => return true,
        };

        self.input_frame_queue
            .push_back(Box::new(self.frame_holder.clone()));
        true
    }

    pub fn prep_frame(&mut self) {
        if self.input_frame_queue.len() < max(self.buffer_size.saturating_sub(30), self.buffer_size)
        {
            self.fill_buffer();
        }
    }

    pub fn ensure_one_frame(&mut self) -> bool {
        if self.input_frame_queue.is_empty() {
            return self.get_next_image();
        }
        true
    }

    pub fn pop_frame(&mut self) -> Option<Box<Mat>> {
        self.input_frame_queue.pop_front()
    }
}
