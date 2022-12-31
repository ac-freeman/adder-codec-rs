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
use rayon::iter::ParallelIterator;
use rayon::iter::{IndexedParallelIterator, IntoParallelRefMutIterator};

use opencv::core::{Mat, CV_8U};
use opencv::{prelude::*, Result};

use bumpalo::Bump;
use ndarray::{Array3, Axis};

use rayon::iter::IntoParallelIterator;
use rayon::{current_num_threads, ThreadPool};
use std::cmp::max;
use std::time::Instant;

use crate::framer::scale_intensity::FrameValue;
use crate::transcoder::event_pixel_tree::Intensity32;
use tokio::runtime::Runtime;

pub struct Framed {}
pub struct Raw {}

#[derive(PartialEq, Clone, Copy)]
pub enum DavisTranscoderMode {
    Framed,
    RawDavis,
    RawDvs,
}

/// Attributes of a framed video -> ADΔER transcode
pub struct DavisSource {
    reconstructor: Reconstructor,
    pub(crate) input_frame_scaled: Mat,
    pub(crate) video: Video,
    image_8u: Mat,
    thread_pool_edi: ThreadPool,
    _thread_pool_integration: ThreadPool,
    dvs_c: f64,
    dvs_events_before: Option<Vec<DvsEvent>>,
    dvs_events_after: Option<Vec<DvsEvent>>,
    pub start_of_frame_timestamp: Option<i64>,
    pub end_of_frame_timestamp: Option<i64>,
    pub rt: Runtime,
    pub dvs_last_timestamps: Array3<i64>,
    pub dvs_last_ln_val: Array3<f64>,
    optimize_adder_controller: bool,
    pub mode: DavisTranscoderMode, // phantom: PhantomData<T>,
}

unsafe impl Sync for DavisSource {}

impl DavisSource {
    /// Initialize the framed source and read first frame of source, in order to get `height`
    /// and `width` and initialize [`Video`]
    pub fn new(
        reconstructor: Reconstructor,
        output_events_filename: Option<String>,
        tps: DeltaT,
        tpf: f64,
        delta_t_max: DeltaT,
        show_display_b: bool,
        adder_c_thresh_pos: u8,
        adder_c_thresh_neg: u8,
        optimize_adder_controller: bool,
        rt: Runtime,
        mode: DavisTranscoderMode,
        write_out: bool,
    ) -> Result<DavisSource> {
        let video = Video::new(
            reconstructor.width as u16,
            reconstructor.height as u16,
            64,
            output_events_filename,
            1,
            tps,
            // ref_time is set based on the reconstructor's output_fps, which is the user-set
            // rate OR might be higher if the first APS image in the video has a shorter exposure
            // time than expected
            tpf as u32,
            delta_t_max,
            DecimationMode::Manual,
            write_out,
            true,
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

        let timestamps =
            vec![0.0_f64; video.height as usize * video.width as usize * video.channels as usize];

        let dvs_last_ln_val: Array3<f64> = Array3::from_shape_vec(
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
            _thread_pool_integration: thread_pool_integration,
            dvs_c: 0.15,
            dvs_events_before: None,
            dvs_events_after: None,
            start_of_frame_timestamp: None,
            end_of_frame_timestamp: None,
            rt,
            dvs_last_timestamps,
            dvs_last_ln_val,
            optimize_adder_controller,
            mode,
        };
        Ok(davis_source)
    }

    pub fn integrate_dvs_events<F: Fn(i64, i64) -> bool + Send + 'static + std::marker::Sync>(
        &mut self,
        dvs_events: &Vec<DvsEvent>,
        frame_timestamp: &i64,
        event_check: F,
    ) {
        let mut dvs_chunks: [Vec<DvsEvent>; 4] = [
            Vec::with_capacity(100000),
            Vec::with_capacity(100000),
            Vec::with_capacity(100000),
            Vec::with_capacity(100000),
        ];

        let mut chunk_idx;
        for dvs_event in dvs_events {
            chunk_idx = dvs_event.y() as usize / (self.video.height as usize / 4);
            dvs_chunks[chunk_idx].push(*dvs_event);
        }

        let chunk_rows = self.video.height as usize / 4;
        // let px_per_chunk: usize =
        //     self.video.chunk_rows * self.video.width as usize * self.video.channels as usize;
        let big_buffer: Vec<Vec<Event>> = self
            .video
            .event_pixel_trees
            .axis_chunks_iter_mut(Axis(0), chunk_rows)
            .into_par_iter()
            .zip(
                self.dvs_last_ln_val
                    .axis_chunks_iter_mut(Axis(0), chunk_rows)
                    .into_par_iter()
                    .zip(
                        self.dvs_last_timestamps
                            .axis_chunks_iter_mut(Axis(0), chunk_rows)
                            .into_par_iter(),
                    ),
            )
            .enumerate()
            .map(
                |(
                    chunk_idx,
                    (mut px_chunk, (mut dvs_last_ln_val_chunk, mut dvs_last_timestamps_chunk)),
                )| {
                    let mut buffer: Vec<Event> = Vec::with_capacity(100000);

                    for event in &dvs_chunks[chunk_idx] {
                        // Ignore events occuring during the deblurred frame's
                        // effective exposure time
                        if event_check(event.t(), *frame_timestamp) {
                            let px = &mut px_chunk
                                [[(event.y() as usize) % chunk_rows, event.x() as usize, 0]];
                            let base_val = px.base_val;
                            let last_val_ln = &mut dvs_last_ln_val_chunk
                                [[(event.y() as usize) % chunk_rows, event.x() as usize, 0]];
                            let last_val = (last_val_ln.exp() - 1.0) * 255.0;

                            // in microseconds (1 million per second)

                            let delta_t_micro = event.t()
                                - dvs_last_timestamps_chunk
                                    [[event.y() as usize % chunk_rows, event.x() as usize, 0]];
                            let ticks_per_micro = self.video.tps as f32 / 1e6;
                            let delta_t_ticks = delta_t_micro as f32 * ticks_per_micro;
                            if delta_t_ticks <= 0.0 {
                                continue; // TODO: do better
                            }
                            assert!(delta_t_ticks > 0.0);
                            let _frame_delta_t = self.video.ref_time;
                            // integrate_for_px(px, base_val, &frame_val, 0.0, 0.0, Mode::FramePerfect, &mut vec![], &0, &0, &0)

                            // First, integrate the previous value enough to fill the time since then
                            let first_integration = ((last_val as Intensity32)
                                / self.video.ref_time as f32
                                * delta_t_ticks)
                                .max(0.0);
                            if px.need_to_pop_top {
                                buffer.push(px.pop_top_event(Some(first_integration)));
                            }

                            px.integrate(
                                first_integration,
                                delta_t_ticks,
                                &Continuous,
                                &self.video.delta_t_max,
                                &self.video.ref_time,
                            );
                            if px.need_to_pop_top {
                                buffer.push(px.pop_top_event(Some(first_integration)));
                            }

                            ///////////////////////////////////////////////////////
                            // Then, integrate a tiny amount of the next intensity
                            // let mut frame_val = (base_val as f64);
                            // let mut lat_frame_val = (frame_val / 255.0).ln();

                            *last_val_ln += match event.on() {
                                true => self.dvs_c,
                                false => -self.dvs_c,
                            };
                            let mut frame_val = (last_val_ln.exp() - 1.0) * 255.0;
                            clamp_u8(&mut frame_val, last_val_ln);

                            let frame_val_u8 = frame_val as u8; // TODO: don't let this be lossy here

                            if frame_val_u8 < base_val.saturating_sub(self.video.c_thresh_neg)
                                || frame_val_u8 > base_val.saturating_add(self.video.c_thresh_pos)
                            {
                                px.pop_best_events(None, &mut buffer);
                                px.base_val = frame_val_u8;

                                // If continuous mode and the D value needs to be different now
                                match px.set_d_for_continuous(frame_val as Intensity32) {
                                    None => {}
                                    Some(event) => buffer.push(event),
                                };
                            }

                            dvs_last_timestamps_chunk
                                [[event.y() as usize % chunk_rows, event.x() as usize, 0]] =
                                event.t();
                        }
                    }

                    buffer
                },
            )
            .collect();
        // exact_chunks_iter_mut(Dim([
        //     self.video.width as usize,
        //     self.video.height as usize / 4,
        //     1,
        // ]));

        // slices.

        // Using a macro so that CLion still pretty prints correctly

        if self.video.write_out {
            self.video.stream.encode_events_events(&big_buffer);
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
            .zip(
                self.dvs_last_ln_val
                    .axis_chunks_iter_mut(Axis(0), self.video.chunk_rows)
                    .into_par_iter(),
            )
            .enumerate()
            .map(|(chunk_idx, (mut chunk_px, mut chunk_ln_val))| {
                let mut buffer: Vec<Event> = Vec::with_capacity(px_per_chunk);
                let bump = Bump::new();
                let base_val = bump.alloc(0);
                let px_idx = bump.alloc(0);
                let frame_val = bump.alloc(0);

                for (chunk_px_idx, (px, last_val_ln)) in
                    chunk_px.iter_mut().zip(chunk_ln_val.iter_mut()).enumerate()
                {
                    *px_idx = chunk_px_idx + px_per_chunk * chunk_idx;

                    let last_val = (last_val_ln.exp() - 1.0) * 255.0;

                    *base_val = px.base_val;
                    *frame_val = last_val as u8;

                    let ticks_per_micro = self.video.tps as f32 / 1e6;

                    let delta_t_micro = self.start_of_frame_timestamp.unwrap()
                        - self.dvs_last_timestamps[[px.coord.y as usize, px.coord.x as usize, 0]];

                    let delta_t_ticks = delta_t_micro as f32 * ticks_per_micro;
                    if delta_t_ticks <= 0.0 {
                        continue;
                    }
                    assert!(delta_t_ticks > 0.0);
                    // assert_eq!(
                    //     self.end_of_frame_timestamp.unwrap()
                    //         - self.start_of_frame_timestamp.unwrap(),
                    //     (self.video.ref_time as f32 / ticks_per_micro as f32) as i64
                    // );

                    let integration =
                        ((last_val / self.video.ref_time as f64) * delta_t_ticks as f64).max(0.0);
                    assert!(integration >= 0.0);

                    integrate_for_px(
                        px,
                        base_val,
                        frame_val,
                        integration as f32,
                        delta_t_ticks,
                        Continuous,
                        &mut buffer,
                        &self.video.c_thresh_pos,
                        &self.video.c_thresh_neg,
                        &self.video.delta_t_max,
                        &self.video.ref_time,
                    );
                    if px.need_to_pop_top {
                        buffer.push(px.pop_top_event(Some(integration as f32)));
                    }
                }
                buffer
            })
            .collect();

        if self.video.write_out {
            self.video.stream.encode_events_events(&big_buffer);
        }

        let db = self.video.instantaneous_frame.data_bytes_mut().unwrap();

        // TODO: split off into separate function
        // TODO: When there's full support for various bit-depth sources, modify this accordingly
        let practical_d_max =
            fast_math::log2_raw(255.0 * (self.video.delta_t_max / self.video.ref_time) as f32);
        db.par_iter_mut().enumerate().for_each(|(idx, val)| {
            let y = idx / (self.video.width as usize * self.video.channels);
            let x = (idx % (self.video.width as usize * self.video.channels)) / self.video.channels;
            let c = idx % self.video.channels;
            *val = match self.video.event_pixel_trees[[y, x, c]].arena[0].best_event {
                Some(event) => u8::get_frame_value(
                    &event,
                    SourceType::U8,
                    self.video.ref_time as DeltaT,
                    practical_d_max,
                    self.video.delta_t_max,
                    self.video.instantaneous_view_mode,
                ),
                None => *val,
            };
        });
        if self.video.show_live {
            show_display("instance", &self.video.instantaneous_frame, 1, &self.video);
        }
    }

    fn control_latency(&mut self, opt_timestamp: Option<Instant>) {
        if self.optimize_adder_controller {
            match opt_timestamp {
                None => {}
                Some(timestamp) => {
                    let latency = (Instant::now() - timestamp).as_millis();
                    match latency as f64 >= self.reconstructor.target_latency * 3.0 {
                        true => {
                            self.video.c_thresh_pos = self.video.c_thresh_pos.saturating_add(1);
                            self.video.c_thresh_neg = self.video.c_thresh_neg.saturating_add(1);
                        }
                        false => {
                            self.video.c_thresh_pos = self.video.c_thresh_pos.saturating_sub(1);
                            self.video.c_thresh_neg = self.video.c_thresh_neg.saturating_sub(1);
                        }
                    }
                    eprintln!(
                        "    adder latency = {}, adder c = {}",
                        latency, self.video.c_thresh_pos
                    );
                }
            }
        }
    }

    pub fn get_reconstructor(&self) -> &Reconstructor {
        &self.reconstructor
    }

    pub fn get_reconstructor_mut(&mut self) -> &mut Reconstructor {
        &mut self.reconstructor
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
            DavisTranscoderMode::RawDavis => true,
            DavisTranscoderMode::RawDvs => true,
        };
        let mat_opt = self.rt.block_on(get_next_image(
            &mut self.reconstructor,
            &self.thread_pool_edi,
            with_events,
        ));
        match mat_opt {
            None => {
                // We've reached the end of the input. Forcibly pop the last event from each pixel.
                println!("Popping remaining events");
                let px_per_chunk: usize = self.video.chunk_rows
                    * self.video.width as usize
                    * self.video.channels as usize;
                let big_buffer: Vec<Vec<Event>> = self
                    .video
                    .event_pixel_trees
                    .axis_chunks_iter_mut(Axis(0), self.video.chunk_rows)
                    .into_par_iter()
                    .enumerate()
                    .map(|(_chunk_idx, mut chunk)| {
                        let mut buffer: Vec<Event> = Vec::with_capacity(px_per_chunk);
                        for (_, px) in chunk.iter_mut().enumerate() {
                            px.pop_best_events(None, &mut buffer);
                        }
                        buffer
                    })
                    .collect();

                if self.video.write_out {
                    self.video.stream.encode_events_events(&big_buffer);
                }

                return Err(SourceError::NoData);
            }
            Some((
                mat,
                opt_timestamp,
                Some((c, events_before, events_after, img_start_ts, img_end_ts)),
            )) => {
                self.control_latency(opt_timestamp);

                self.input_frame_scaled = mat;
                self.dvs_c = c;
                self.dvs_events_before = Some(events_before);
                self.dvs_events_after = Some(events_after);
                self.start_of_frame_timestamp = Some(img_start_ts);
                self.end_of_frame_timestamp = Some(img_end_ts);
                self.video.ref_time_divisor =
                    (img_end_ts - img_start_ts) as f64 / self.video.ref_time as f64;
            }
            Some((mat, opt_timestamp, None)) => {
                self.control_latency(opt_timestamp);
                self.input_frame_scaled = mat;
            }
        }
        if with_events {
            if self.video.in_interval_count == 0 {
                self.dvs_last_timestamps.par_map_inplace(|ts| {
                    *ts = self.start_of_frame_timestamp.unwrap();
                });
            } else {
                self.integrate_dvs_events(
                    &self.dvs_events_before.as_ref().unwrap().clone(),
                    &self.start_of_frame_timestamp.unwrap(),
                    &check_dvs_before,
                );
                self.integrate_frame_gaps();
            }
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
        let mut tmp = self.image_8u.clone();
        let mat_integration_time = match self.mode {
            DavisTranscoderMode::Framed => self.video.ref_time as f32,
            DavisTranscoderMode::RawDavis => {
                (self.end_of_frame_timestamp.unwrap() - self.start_of_frame_timestamp.unwrap())
                    as f32
            }
            DavisTranscoderMode::RawDvs => {
                // TODO: Note how c is fixed here, since we don't have a mechanism for determining
                // its value
                self.dvs_c = 0.15;
                match tmp.data_bytes_mut() {
                    Ok(bytes) => {
                        for byte in bytes {
                            *byte = 0;
                        }
                    }
                    Err(_) => {
                        panic!("Mat error")
                    }
                }
                0.0
            }
        };

        let ret = thread_pool.install(|| {
            self.video
                .integrate_matrix(tmp, mat_integration_time, Continuous, view_interval)
        });

        unsafe {
            for (idx, val) in self.dvs_last_ln_val.iter_mut().enumerate() {
                let px = self
                    .input_frame_scaled
                    .at_unchecked::<f64>(idx as i32)
                    .unwrap();
                match self.mode {
                    DavisTranscoderMode::Framed => {
                        *val = px.ln_1p();
                    }
                    DavisTranscoderMode::RawDavis => {
                        *val = px.ln_1p();
                    }
                    DavisTranscoderMode::RawDvs => {
                        *val = 0.5_f64.ln_1p();
                    }
                }
            }
        }

        if with_events {
            self.dvs_last_timestamps.par_map_inplace(|ts| {
                *ts = self.end_of_frame_timestamp.unwrap();
            });

            self.integrate_dvs_events(
                &self.dvs_events_after.as_ref().unwrap().clone(),
                &self.end_of_frame_timestamp.unwrap(),
                &check_dvs_after,
            );
        }

        ret
    }

    fn get_video_mut(&mut self) -> &mut Video {
        &mut self.video
    }

    fn get_video(&self) -> &Video {
        &self.video
    }
}

fn check_dvs_before(dvs_event_t: i64, timestamp_before: i64) -> bool {
    dvs_event_t < timestamp_before
}

fn check_dvs_after(dvs_event_t: i64, timestamp_after: i64) -> bool {
    dvs_event_t > timestamp_after
}

fn clamp_u8(frame_val: &mut f64, last_val_ln: &mut f64) {
    if *frame_val <= 0.0 {
        *frame_val = 0.0;
        *last_val_ln = 0.0; // = 0.0_f64.ln_1p();
    } else if *frame_val > 255.0 {
        *frame_val = 255.0;
        *last_val_ln = 255.0_f64.ln_1p();
    }
}

pub async fn get_next_image(
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