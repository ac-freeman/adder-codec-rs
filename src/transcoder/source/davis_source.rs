use crate::transcoder::d_controller::DecimationMode;
use crate::transcoder::event_pixel_tree::Mode::Continuous;
use crate::transcoder::source::video::SourceError::BufferEmpty;
use crate::transcoder::source::video::{
    integrate_for_px, show_display, Source, SourceError, Video,
};
use crate::SourceCamera::DavisU8;
use crate::{Codec, DeltaT, Event, SourceType};
use aedat::events_generated::Event as DvsEvent;
use davis_edi_rs::util::reconstructor::{IterVal, Reconstructor};
use rayon::iter::IndexedParallelIterator;
use rayon::iter::ParallelIterator;
use std::marker::PhantomData;

use opencv::core::{Mat, CV_8U};
use opencv::{prelude::*, Result};

use bumpalo::Bump;
use ndarray::{Array3, Axis};
use rayon::iter::IntoParallelIterator;
use rayon::{current_num_threads, ThreadPool};
use std::cmp::max;

use crate::framer::scale_intensity::FrameValue;
use crate::transcoder::event_pixel_tree::Intensity32;
use tokio::runtime::Runtime;

// https://stackoverflow.com/questions/51344951/how-do-you-unwrap-a-result-on-ok-or-return-from-the-function-on-err
macro_rules! unwrap_or_return {
    ( $e:expr ) => {
        match $e {
            Some(x) => x,
            None => return,
        }
    };
}

pub struct Framed {}
pub struct Raw {}

pub enum DavisTranscoderMode {
    Framed,
    Raw,
}

/// Attributes of a framed video -> ADΔER transcode
pub struct DavisSource {
    reconstructor: Reconstructor,
    pub(crate) input_frame_scaled: Mat,
    pub(crate) video: Video,
    image_8u: Mat,
    thread_pool_edi: ThreadPool,
    thread_pool_integration: ThreadPool,
    dvs_events: Option<Vec<DvsEvent>>,
    end_of_frame_timestamp: Option<i64>,
    pub rt: Runtime,
    dvs_last_timestamps: Array3<i64>,
    mode: DavisTranscoderMode, // phantom: PhantomData<T>,
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
        mode: DavisTranscoderMode,
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
            adder_c_thresh_pos,
            adder_c_thresh_neg,
        );
        let thread_pool_edi = rayon::ThreadPoolBuilder::new()
            .num_threads(max(current_num_threads() - 4, 1))
            .build()
            .unwrap();
        let thread_pool_integration = rayon::ThreadPoolBuilder::new()
            .num_threads(max(4, 1))
            .build()
            .unwrap();

        let timestamps =
            vec![0_i64; video.height as usize * video.width as usize * video.channels as usize];

        let dvs_last_timestamps: Array3<i64> = Array3::from_shape_vec(
            (video.height.into(), video.width.into(), video.channels),
            timestamps,
        )
        .unwrap();

        let davis_source = DavisSource {
            reconstructor,
            input_frame_scaled: Mat::default(),
            video,
            image_8u: Mat::default(),
            thread_pool_edi,
            thread_pool_integration,
            dvs_events: None,
            end_of_frame_timestamp: None,
            rt,
            dvs_last_timestamps,
            mode,
        };
        Ok(davis_source)
    }

    // TODO: need to return the events for simultaneously reframing?
    pub fn integrate_dvs_events(&mut self) {
        // Using a macro so that CLion still pretty prints correctly
        let mut buffer: Vec<Event> = Vec::with_capacity(500); // TODO: experiment with capacity
        let dvs_events = unwrap_or_return!(self.dvs_events.as_ref());
        let end_of_frame_timestamp = unwrap_or_return!(self.end_of_frame_timestamp.as_ref());
        for event in dvs_events.iter() {
            if event.t() > *end_of_frame_timestamp {
                let px =
                    &mut self.video.event_pixel_trees[[event.y() as usize, event.x() as usize, 0]];
                let base_val = px.base_val;

                // in microseconds (1 million per second)

                let delta_t_micro = event.t()
                    - self.dvs_last_timestamps[[event.y() as usize, event.x() as usize, 0]];
                let ticks_per_micro = self.video.tps as f32 / 1e6;
                let delta_t_ticks = delta_t_micro as f32 * ticks_per_micro;
                if delta_t_ticks <= 0.0 {
                    continue; // TODO: do better
                }
                assert!(delta_t_ticks > 0.0);
                let frame_delta_t = self.video.ref_time;
                // integrate_for_px(px, base_val, &frame_val, 0.0, 0.0, Mode::FramePerfect, &mut vec![], &0, &0, &0)

                // First, integrate the previous value enough to fill the time since then
                let first_integration =
                    (base_val as Intensity32) / self.video.ref_time as f32 * delta_t_ticks;

                if px.need_to_pop_top {
                    buffer.push(px.pop_top_event(Some(first_integration)));
                }

                px.integrate(
                    first_integration,
                    delta_t_ticks,
                    &Continuous,
                    &self.video.delta_t_max,
                );

                ///////////////////////////////////////////////////////
                // Then, integrate a tiny amount of the next intensity
                let mut frame_val = (base_val as Intensity32);
                frame_val += match event.on() {
                    true => 0.0,
                    false => -0.0, // TODO: temporary, just for debugging setup
                };
                let frame_val = frame_val as u8;

                if frame_val < base_val.saturating_sub(self.video.c_thresh_neg)
                    || frame_val > base_val.saturating_add(self.video.c_thresh_pos)
                {
                    px.pop_best_events(None, &mut buffer);
                    px.base_val = frame_val;

                    // If continuous mode and the D value needs to be different now
                    match px.set_d_for_continuous(0.0) {
                        // TODO: This may cause issues if events are very close together in time
                        None => {}
                        Some(event) => buffer.push(event),
                    };
                }

                // px.integrate(
                //     *frame_val as Intensity32,
                //     ref_time,
                //     &pixel_tree_mode,
                //     &delta_t_max,
                // );

                self.dvs_last_timestamps[[event.y() as usize, event.x() as usize, 0]] = event.t();
            }
        }

        if self.video.write_out {
            self.video.stream.encode_events(&buffer);
        }
    }

    fn integrate_frame_gaps(&mut self) {
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

                    *frame_val = *base_val;

                    // TODO: Also need start of video timestamp
                    let ticks_per_micro = self.video.tps as f32 / 1e6;
                    let tmp_0 = self.end_of_frame_timestamp.unwrap();
                    let tmp_1 = (self.video.ref_time as f32 * ticks_per_micro) as i64;
                    let tmp_2 =
                        self.dvs_last_timestamps[[px.coord.y as usize, px.coord.x as usize, 0]];
                    let delta_t_micro = self.end_of_frame_timestamp.unwrap()
                        - (self.video.ref_time as f32 / ticks_per_micro) as i64
                        - self.dvs_last_timestamps[[px.coord.y as usize, px.coord.x as usize, 0]];

                    let delta_t_ticks = delta_t_micro as f32 * ticks_per_micro;
                    if delta_t_ticks <= 0.0 {
                        continue; // TODO: a hacky way around the problem. Need to also get the frame start timestamp
                    }
                    assert!(delta_t_ticks > 0.0);

                    let integration =
                        (*base_val as Intensity32) / self.video.ref_time as f32 * delta_t_ticks;
                    assert!(integration >= 0.0);
                    if integration > 0.0 {
                        println!("");
                    }

                    integrate_for_px(
                        px,
                        &mut base_val,
                        frame_val,
                        integration, // In this case, frame val is the same as intensity to integrate
                        delta_t_ticks,
                        Continuous,
                        &mut buffer,
                        &self.video.c_thresh_pos,
                        &self.video.c_thresh_neg,
                        &self.video.delta_t_max,
                    )
                }
                buffer
            })
            .collect();

        if self.video.write_out {
            self.video.stream.encode_events_events(&big_buffer);
        }

        // TODO: temporary
        for r in 0..self.video.height as i32 {
            for c in 0..self.video.width as i32 {
                let inst_px: &mut u8 = self.video.instantaneous_frame.at_2d_mut(r, c).unwrap();
                let px = &mut self.video.event_pixel_trees[[r as usize, c as usize, 0]];
                *inst_px = match px.arena[0].best_event.clone() {
                    Some(event) => {
                        u8::get_frame_value(&event, SourceType::U8, self.video.ref_time as DeltaT)
                    }
                    None => 0,
                };
            }
        }
        show_display("instance", &self.video.instantaneous_frame, 1, &self.video);
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

        let with_events = match self.mode {
            DavisTranscoderMode::Framed => false,
            DavisTranscoderMode::Raw => true,
        };
        let mat_opt = self.rt.block_on(get_next_image(
            &mut self.reconstructor,
            &self.thread_pool_edi,
            with_events,
        ));
        match mat_opt {
            None => {
                return Err(SourceError::NoData);
            }
            Some((mat, Some((events, timestamp)))) => {
                self.input_frame_scaled = mat;
                self.dvs_events = Some(events);
                self.end_of_frame_timestamp = Some(timestamp);
                // self.dvs_last_timestamps.par_map_inplace(|ts| {
                //     *ts = timestamp;
                // });
            }
            Some((mat, None)) => {
                self.input_frame_scaled = mat;
            }
        }

        if self.video.in_interval_count == 0 {
            self.dvs_last_timestamps.par_map_inplace(|ts| {
                *ts = self.end_of_frame_timestamp.unwrap();
            });
        } else {
            self.integrate_frame_gaps();
        }

        if self.input_frame_scaled.empty() {
            eprintln!("End of video");
            return Err(BufferEmpty);
        }

        self.input_frame_scaled
            .convert_to(&mut self.image_8u, CV_8U, 255.0, 0.0)
            .unwrap();

        // While `input_frame_scaled` may not be continuous (which would cause problems with
        // iterating over the pixels), cloning it ensures that it is made continuous.
        // https://stackoverflow.com/questions/33665241/is-opencv-matrix-data-guaranteed-to-be-continuous
        let tmp = self.image_8u.clone();
        thread_pool.install(|| {
            self.video
                .integrate_matrix(tmp, self.video.ref_time as f32, Continuous, view_interval)
        })
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
    with_events: bool,
) -> Option<IterVal> {
    thread_pool
        .install(|| async {
            match reconstructor.next(with_events).await {
                None => {
                    println!("\nFinished!");
                    None
                }
                Some(res) => match res {
                    Ok(a) => Some(a),
                    Err(_) => {
                        panic!("No image")
                    }
                },
            }
        })
        .await
}
