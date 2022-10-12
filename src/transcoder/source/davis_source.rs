
use crate::transcoder::d_controller::DecimationMode;
use crate::transcoder::event_pixel_tree::Mode::{Continuous};
use crate::transcoder::event_pixel_tree::{Intensity_32, D};
use crate::transcoder::source::video::SourceError::BufferEmpty;
use crate::transcoder::source::video::{show_display, Source, SourceError, Video};
use crate::SourceCamera::DavisU8;
use crate::{Codec, DeltaT, Event};
use bumpalo::Bump;
use davis_edi_rs::util::reconstructor::Reconstructor;

use ndarray::{Axis};
use opencv::core::{Mat, CV_8U};
use opencv::{prelude::*, Result};
use rayon::iter::IntoParallelIterator;
use rayon::iter::ParallelIterator;
use rayon::iter::{IndexedParallelIterator};
use rayon::{current_num_threads, ThreadPool};
use std::cmp::max;

use tokio::runtime::Runtime;

/// Attributes of a framed video -> ADΔER transcode
pub struct DavisSource {
    reconstructor: Reconstructor,
    pub(crate) input_frame_scaled: Mat,
    c_thresh_pos: u8,
    c_thresh_neg: u8,

    pub(crate) video: Video,
    image_8u: Mat,
    thread_pool_edi: ThreadPool,
    thread_pool_integration: ThreadPool,
    pub rt: Runtime,
}

impl DavisSource {
    /// Initialize the framed source and read first frame of source, in order to get `height`
    /// and `width` and initialize [`Video`]
    pub fn new(
        reconstructor: Reconstructor,
        output_events_filename: Option<String>,
        tps: DeltaT,
        delta_t_max: DeltaT,
        show_display_b: bool,
        adder_c_thresh_pos: u8,
        adder_c_thresh_neg: u8,
        rt: Runtime,
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
        let thread_pool_edi = rayon::ThreadPoolBuilder::new()
            .num_threads(max(current_num_threads() - 4, 1))
            .build()
            .unwrap();
        let thread_pool_integration = rayon::ThreadPoolBuilder::new()
            .num_threads(max(4, 1))
            .build()
            .unwrap();

        let davis_source = DavisSource {
            reconstructor,
            input_frame_scaled: Mat::default(),
            c_thresh_pos: adder_c_thresh_pos,
            c_thresh_neg: adder_c_thresh_neg,
            video,
            image_8u: Mat::default(),
            thread_pool_edi,
            thread_pool_integration,
            rt,
        };
        Ok(davis_source)
    }
}

impl Source for DavisSource {
    fn consume(
        &mut self,
        view_interval: u32,
        thread_pool: &ThreadPool,
    ) -> std::result::Result<Vec<Vec<Event>>, SourceError> {
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

        let mat_opt = self.rt.block_on(get_next_image(
            &mut self.reconstructor,
            &self.thread_pool_edi,
        ));
        if mat_opt.is_none() {
            return Err(SourceError::NoData);
        }
        self.input_frame_scaled = mat_opt.unwrap();

        // Copied from framed_source.rs. TODO: break out the common code and share it
        if self.video.in_interval_count == 0 {
            let frame_arr = self.input_frame_scaled.data_bytes().unwrap();

            self.video.event_pixel_trees.par_map_inplace(|px| {
                let idx = px.coord.y as usize * self.video.width as usize * self.video.channels
                    + px.coord.x as usize * self.video.channels
                    + px.coord.c.unwrap_or(0) as usize;
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

        let mut image_8u = Mat::default();

        // While `input_frame_scaled` may not be continuous (which would cause problems with
        // iterating over the pixels), cloning it ensures that it is made continuous.
        // https://stackoverflow.com/questions/33665241/is-opencv-matrix-data-guaranteed-to-be-continuous
        self.input_frame_scaled
            .clone()
            .convert_to(&mut image_8u, CV_8U, 255.0, 0.0)
            .unwrap();

        thread_pool.install(|| self.integrate_matrix(image_8u, self.video.ref_time as f32))
    }

    fn integrate_matrix(
        &mut self,
        matrix: Mat,
        ref_time: f32,
    ) -> std::result::Result<Vec<Vec<Event>>, SourceError> {
        let frame_arr: &[u8] = matrix.data_bytes().unwrap();
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
                        px.pop_best_events(None, &mut buffer);
                        px.base_val = *frame_val;

                        // If continuous mode and the D value needs to be different now
                        // TODO: make it modular
                        match px.set_d_for_continuous(*frame_val as Intensity_32) {
                            None => {}
                            Some(event) => buffer.push(event),
                        };
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

        Ok(big_buffer)
    }

    fn get_video_mut(&mut self) -> &mut Video {
        &mut self.video
    }

    fn get_video(&self) -> &Video {
        &self.video
    }
}

async fn get_next_image(
    reconstructor: &mut Reconstructor,
    thread_pool: &ThreadPool,
) -> Option<Mat> {
    thread_pool
        .install(|| async {
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
        })
        .await
}
