use crate::framer::scale_intensity::{event_to_intensity, FrameValue};
use crate::transcoder::d_controller::DecimationMode;
use crate::transcoder::event_pixel_tree::Mode::{Continuous, FramePerfect};
use crate::transcoder::event_pixel_tree::{Intensity_32, D};
use crate::transcoder::source::video::SourceError::BufferEmpty;
use crate::transcoder::source::video::{show_display, Source, SourceError, Video};
use crate::SourceCamera::DavisU8;
use crate::{Codec, DeltaT, Event, SourceType};
use bumpalo::Bump;
use davis_edi_rs::util::reconstructor::Reconstructor;
use davis_edi_rs::*;
use ndarray::Axis;
use opencv::core::Mat;
use opencv::{imgproc, prelude::*, videoio, Result};
use rayon::iter::IndexedParallelIterator;
use rayon::iter::IntoParallelIterator;
use rayon::iter::ParallelIterator;
use std::sync::mpsc::{Receiver, Sender};

/// Attributes of a framed video -> ADΔER transcode
pub struct DavisSource {
    reconstructor: Reconstructor,
    pub(crate) input_frame_scaled: Mat,
    c_thresh_pos: u8,
    c_thresh_neg: u8,

    pub(crate) video: Video,
    image_8u: Mat,
}

impl DavisSource {
    /// Initialize the framed source and read first frame of source, in order to get `height`
    /// and `width` and initialize [`Video`]
    pub fn new(
        mut reconstructor: Reconstructor,
        output_events_filename: Option<String>,
        tps: DeltaT,
        delta_t_max: DeltaT,
        show_display_b: bool,
    ) -> Result<DavisSource> {
        let video = Video::new(
            reconstructor.width as u16,
            reconstructor.height as u16,
            64,
            output_events_filename,
            1,
            tps,
            (tps as f64 / reconstructor.output_fps) as u32,
            delta_t_max,
            DecimationMode::Manual,
            true, // TODO
            true, // TODO
            show_display_b,
            DavisU8,
        );
        let davis_source = DavisSource {
            reconstructor,
            input_frame_scaled: Mat::default(),
            c_thresh_pos: 15, // TODO
            c_thresh_neg: 15, // TODO
            video,
            image_8u: Mat::default(),
        };
        Ok(davis_source)
    }
}

impl Source for DavisSource {
    fn consume(&mut self, view_interval: u32) -> std::result::Result<Vec<Vec<Event>>, SourceError> {
        // Attempting new method for integration without requiring a buffer. Could be implemented
        // for framed source just as easily
        // Keep running integration starting at D=log_2(current_frame) + 1
        // --If exceeds 2^D, then store in the pixel object what that event would be.
        // --Then keep track of two branches:
        // ----1: continuing the integration for D + 1
        // ----2: assume that event fired, and integrate for a new event
        // ---------But this could branch too... some sort of binary tree of pixel objects?
        // ---------if (1) fills up for the higher D, then delete (2) and
        //          create a new branch for (2)

        let rt = tokio::runtime::Runtime::new().unwrap();
        let mat_opt = rt.block_on(get_next_image(&mut self.reconstructor));
        if mat_opt.is_none() {
            return Err(SourceError::NoData);
        }
        self.input_frame_scaled = mat_opt.unwrap();

        // Copied from framed_source.rs. TODO: break out the common code and share it
        if self.video.in_interval_count == 0 {
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

        let mut frame_arr = Vec::with_capacity(
            self.video.width as usize * self.video.height as usize * self.video.channels,
        );
        unsafe {
            for idx in 0..self.video.height as i32 * self.video.width as i32 {
                let val: *const u8 =
                    self.input_frame_scaled.at_unchecked(idx).unwrap() as *const u8;
                frame_arr.push(*val);
            }
        }

        // let frame_arr: &[u8] = self.input_frame_scaled.data_bytes().unwrap();

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
                        &FramePerfect,
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
        // TODO: temporary
        for r in 0..self.video.height as i32 {
            for c in 0..self.video.width as i32 {
                let inst_px: &mut u8 = self.video.instantaneous_frame.at_2d_mut(r, c).unwrap();
                let px = &mut self.video.event_pixel_trees[[r as usize, c as usize, 0]];
                *inst_px = match px.arena[0].best_event.clone() {
                    Some(event) => u8::get_frame_value(&event, SourceType::U8, ref_time as DeltaT),
                    None => 0,
                };
            }
        }
        show_display("instance", &self.video.instantaneous_frame, 1, &self.video);

        Ok(big_buffer)
    }

    fn get_video_mut(&mut self) -> &mut Video {
        &mut self.video
    }

    fn get_video(&self) -> &Video {
        &self.video
    }
}

async fn get_next_image(reconstructor: &mut Reconstructor) -> Option<Mat> {
    match reconstructor.next().await {
        None => {
            println!("\nFinished!");
            None
        }
        Some(image) => {
            // frame_count += 1;
            match image {
                Ok(a) => Some(a),
                Err(_) => {
                    panic!("No image")
                }
            }
        }
    }
}
