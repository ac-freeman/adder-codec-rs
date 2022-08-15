use crate::transcoder::event_pixel::{DeltaT, Intensity};
use crate::transcoder::source::video::show_display;
use crate::transcoder::source::video::Source;
use crate::transcoder::source::video::Video;
use crate::{Codec, Coord, Event, D, D_MAX};
use core::default::Default;
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

impl FramedSource {
    /// Initialize the framed source and read first frame of source, in order to get `height`
    /// and `width` and initialize [`Video`]
    pub fn new(
        input_filename: String,
        frame_idx_start: u32,
        ref_time: DeltaT,
        tps: DeltaT,
        delta_t_max: DeltaT,
        scale: f64,
        frame_skip_interval: u8,
        color_input: bool,
        lookahead: bool,
        c_thresh_pos: u8,
        c_thresh_neg: u8,
        write_out: bool,
        show_display_b: bool,
        source_camera: SourceCamera,
    ) -> Result<FramedSource> {
        let channels = match color_input {
            true => 3,
            false => 1,
        };

        let mut cap =
            videoio::VideoCapture::from_file(input_filename.as_str(), videoio::CAP_FFMPEG)?;
        let video_frame_count = cap.get(CAP_PROP_FRAME_COUNT).unwrap();
        assert!(frame_idx_start < video_frame_count as u32);

        cap.set(CAP_PROP_POS_FRAMES, frame_idx_start as f64)
            .unwrap();

        let mut cap_lookahead =
            videoio::VideoCapture::from_file(input_filename.as_str(), videoio::CAP_FFMPEG).unwrap();
        init_lookahead(frame_idx_start, lookahead, &mut cap_lookahead);
        assert_eq!(
            ref_time * cap.get(CAP_PROP_FPS).unwrap().round() as u32,
            tps
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
        resize_input(&mut init_frame, &mut init_frame_scaled, scale).unwrap();

        init_frame = init_frame_scaled;

        println!("Width is {}", init_frame.size()?.width);

        // Sanity checks
        // assert!(init_frame.size()?.width > 50);
        // assert!(init_frame.size()?.height > 50);

        let video = Video::new(
            init_frame.size()?.width as u16,
            init_frame.size()?.height as u16,
            "/home/andrew/Downloads/temppp".to_string(),
            channels,
            tps,
            ref_time,
            delta_t_max,
            0,
            write_out,
            show_display_b,
            source_camera,
        );

        let mut frame_buffer: FrameBuffer =
            FrameBuffer::new(120, cap, frame_skip_interval, scale, color_input);
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
            c_thresh_pos,
            c_thresh_neg,
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
            data_bytes.push((*self.lookahead_frames_scaled[i]).data_bytes().unwrap());
        }

        let dtm = self.video.delta_t_max;
        let ref_time = self.video.ref_time as f32;
        let write_out = self.video.write_out;

        let chunk_rows: usize = rayon::current_num_threads();
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

                        px.lookahead_reset();

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
                                if (intensity_sum).log2().floor() as D > current_d {
                                    current_d = (intensity_sum).log2().floor() as D;
                                    ideal_i = i;
                                }
                            } else {
                                break;
                            }
                        }

                        let trans = match ideal_i {
                            0 => Transition {
                                frame_intensity: c_val,
                                sum_intensity_before: intensity_sum,
                                frame_idx: self.video.in_interval_count + 1,
                            },
                            _ => Transition {
                                frame_intensity: 0,
                                sum_intensity_before: intensity_sum,
                                frame_idx: self.video.in_interval_count + ideal_i as u32 + 1,
                            },
                        };
                        px.next_transition = trans;
                        let d_to_set = (intensity_sum).log2().floor() as D;
                        px.d_controller.set_d(current_d);
                        assert!(d_to_set <= D_MAX);
                    }

                    let intensity = frame_arr[px_idx] as Intensity;
                    px.add_intensity(intensity, ref_time, &dtm, &mut buffer, write_out);

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
        if self.input_frame_queue.len() < self.buffer_size - 30 {
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
