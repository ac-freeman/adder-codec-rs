use crate::transcoder::d_controller::DecimationMode;
use crate::transcoder::event_pixel_tree::Mode::Continuous;
use crate::transcoder::source::video::SourceError::BufferEmpty;
use crate::transcoder::source::video::{
    integrate_for_px, show_display, Source, SourceError, Video,
};
use crate::SourceCamera::DavisU8;
use crate::{Codec, DeltaT, Event, PlaneSize, SourceType};
use aedat::events_generated::Event as DvsEvent;
use davis_edi_rs::util::reconstructor::{IterVal, ReconstructionError, Reconstructor};
use rayon::iter::ParallelIterator;
use rayon::iter::{IndexedParallelIterator, IntoParallelRefMutIterator};

use opencv::core::{Mat, CV_8U};
use opencv::prelude::*;

use bumpalo::Bump;
use ndarray::{Array3, Axis};

use rayon::iter::IntoParallelIterator;
use rayon::{current_num_threads, ThreadPool};
use std::cmp::max;
use std::error::Error;

use std::time::Instant;

use crate::framer::scale_intensity::FrameValue;
use crate::raw::stream::Error as StreamError;
use crate::transcoder::event_pixel_tree::Intensity32;
use tokio::runtime::Runtime;

pub struct Framed {}
pub struct Raw {}

#[derive(PartialEq, Clone, Copy)]
pub enum TranscoderMode {
    Framed,
    RawDavis,
    RawDvs,
}

/// Attributes of a framed video -> ADΔER transcode
pub struct Davis {
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
    pub mode: TranscoderMode, // phantom: PhantomData<T>,
}

unsafe impl Sync for Davis {}

impl Davis {
    /// Initialize the framed source and read first frame of source, in order to get `height`
    /// and `width` and initialize [`Video`](crate::Video) struct.
    ///
    /// # Returns
    /// * [`Davis`] - Initialized [`Davis`] object
    pub fn new(
        reconstructor: Reconstructor,
        output_events_filename: Option<String>,
        ticks_per_second: DeltaT,
        ticks_per_frame: f64,
        delta_t_max: DeltaT,
        show_display_b: bool,
        adder_c_thresh_pos: u8,
        adder_c_thresh_neg: u8,
        optimize_adder_controller: bool,
        rt: Runtime,
        mode: TranscoderMode,
        write_out: bool,
    ) -> Result<Davis, Box<dyn Error>> {
        let plane = PlaneSize::new(reconstructor.width, reconstructor.height, 1)?;
        let video = Video::new(
            plane,
            64,
            output_events_filename,
            ticks_per_second,
            // ref_time is set based on the reconstructor's output_fps, which is the user-set
            // rate OR might be higher if the first APS image in the video has a shorter exposure
            // time than expected
            ticks_per_frame as u32,
            delta_t_max,
            DecimationMode::Manual,
            write_out,
            show_display_b,
            DavisU8,
            adder_c_thresh_pos,
            adder_c_thresh_neg,
        )?;
        let thread_pool_edi = rayon::ThreadPoolBuilder::new()
            .num_threads(max(current_num_threads() - 4, 1))
            .build()?;
        let thread_pool_integration = rayon::ThreadPoolBuilder::new()
            .num_threads(max(4, 1))
            .build()?;

        let plane = &video.plane;

        let timestamps = vec![0_i64; video.plane.volume()];

        let dvs_last_timestamps: Array3<i64> = Array3::from_shape_vec(
            (
                plane.height.into(),
                plane.width.into(),
                plane.channels.into(),
            ),
            timestamps,
        )?;

        let timestamps = vec![0.0_f64; video.plane.volume()];

        let dvs_last_ln_val: Array3<f64> = Array3::from_shape_vec(
            (
                plane.height as usize,
                plane.width as usize,
                plane.channels as usize,
            ),
            timestamps,
        )?;

        let davis_source = Davis {
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

    #[allow(clippy::cast_sign_loss)]
    pub fn integrate_dvs_events<F: Fn(i64, i64) -> bool + Send + 'static + std::marker::Sync>(
        &mut self,
        dvs_events: &Vec<DvsEvent>,
        frame_timestamp: &i64,
        event_check: F,
    ) -> Result<(), StreamError> {
        // TODO: not fixed 4 chunks?
        let mut dvs_chunks: [Vec<DvsEvent>; 4] = [
            Vec::with_capacity(100_000),
            Vec::with_capacity(100_000),
            Vec::with_capacity(100_000),
            Vec::with_capacity(100_000),
        ];

        let mut chunk_idx;
        for dvs_event in dvs_events {
            chunk_idx = dvs_event.y() as usize / (self.video.plane.h_usize() / 4);
            dvs_chunks[chunk_idx].push(*dvs_event);
        }

        let chunk_rows = self.video.plane.h_usize() / 4;
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
                    let mut buffer: Vec<Event> = Vec::with_capacity(100_000);

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

                            // First, integrate the previous value enough to fill the time since then
                            let first_integration = ((last_val as Intensity32)
                                / self.video.ref_time as f32
                                * delta_t_ticks)
                                .max(0.0);
                            if px.need_to_pop_top {
                                buffer.push(px.pop_top_event(first_integration));
                            }

                            px.integrate(
                                first_integration,
                                delta_t_ticks,
                                Continuous,
                                self.video.delta_t_max,
                                self.video.ref_time,
                            );
                            if px.need_to_pop_top {
                                buffer.push(px.pop_top_event(first_integration));
                            }

                            ///////////////////////////////////////////////////////
                            // Then, integrate a tiny amount of the next intensity
                            // let mut frame_val = (base_val as f64);
                            // let mut lat_frame_val = (frame_val / 255.0).ln();

                            *last_val_ln += if event.on() { self.dvs_c } else { -self.dvs_c };
                            let mut frame_val = (last_val_ln.exp() - 1.0) * 255.0;
                            clamp_u8(&mut frame_val, last_val_ln);

                            let frame_val_u8 = frame_val as u8; // TODO: don't let this be lossy here

                            if frame_val_u8 < base_val.saturating_sub(self.video.c_thresh_neg)
                                || frame_val_u8 > base_val.saturating_add(self.video.c_thresh_pos)
                            {
                                px.pop_best_events(&mut buffer);
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
            self.video.stream.encode_events_events(&big_buffer)?;
        }
        Ok(())
    }

    #[allow(clippy::cast_possible_truncation)]
    fn integrate_frame_gaps(&mut self) -> Result<(), SourceError> {
        let px_per_chunk: usize = self.video.chunk_rows * self.video.plane.area_wc();

        let start_of_frame_timestamp = match self.start_of_frame_timestamp {
            Some(t) => t,
            None => return Err(SourceError::UninitializedData),
        };

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

                    let delta_t_micro = start_of_frame_timestamp
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

                    let integration = ((last_val / f64::from(self.video.ref_time))
                        * f64::from(delta_t_ticks))
                    .max(0.0);
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
                        buffer.push(px.pop_top_event(integration as f32));
                    }
                }
                buffer
            })
            .collect();

        if self.video.write_out {
            self.video.stream.encode_events_events(&big_buffer)?;
        }

        let db = match self.video.instantaneous_frame.data_bytes_mut() {
            Ok(db) => db,
            Err(e) => return Err(SourceError::OpencvError(e)),
        };

        // TODO: split off into separate function
        // TODO: When there's full support for various bit-depth sources, modify this accordingly
        let practical_d_max =
            fast_math::log2_raw(255.0 * (self.video.delta_t_max / self.video.ref_time) as f32);
        db.par_iter_mut().enumerate().for_each(|(idx, val)| {
            let y = idx / self.video.plane.area_wc();
            let x = (idx % self.video.plane.area_wc()) / self.video.plane.c_usize();
            let c = idx % self.video.plane.c_usize();
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
            show_display("instance", &self.video.instantaneous_frame, 1, &self.video)?;
        }
        Ok(())
    }

    #[allow(clippy::cast_precision_loss)]
    fn control_latency(&mut self, opt_timestamp: Option<Instant>) {
        if self.optimize_adder_controller {
            match opt_timestamp {
                None => {}
                Some(timestamp) => {
                    let latency = timestamp.elapsed().as_millis();
                    if latency as f64 >= self.reconstructor.target_latency * 3.0 {
                        self.video.c_thresh_pos = self.video.c_thresh_pos.saturating_add(1);
                        self.video.c_thresh_neg = self.video.c_thresh_neg.saturating_add(1);
                    } else {
                        self.video.c_thresh_pos = self.video.c_thresh_pos.saturating_sub(1);
                        self.video.c_thresh_neg = self.video.c_thresh_neg.saturating_sub(1);
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

impl Source for Davis {
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
            TranscoderMode::Framed => false,
            TranscoderMode::RawDavis | TranscoderMode::RawDvs => true,
        };
        let mat_opt = self.rt.block_on(get_next_image(
            &mut self.reconstructor,
            &self.thread_pool_edi,
            with_events,
        ));
        match mat_opt {
            Ok(None) => {
                // We've reached the end of the input. Forcibly pop the last event from each pixel.
                println!("Popping remaining events");
                let px_per_chunk: usize = self.video.chunk_rows * self.video.plane.area_wc();
                let big_buffer: Vec<Vec<Event>> = self
                    .video
                    .event_pixel_trees
                    .axis_chunks_iter_mut(Axis(0), self.video.chunk_rows)
                    .into_par_iter()
                    .enumerate()
                    .map(|(_chunk_idx, mut chunk)| {
                        let mut buffer: Vec<Event> = Vec::with_capacity(px_per_chunk);
                        for (_, px) in chunk.iter_mut().enumerate() {
                            px.pop_best_events(&mut buffer);
                        }
                        buffer
                    })
                    .collect();

                if self.video.write_out {
                    self.video.stream.encode_events_events(&big_buffer)?;
                }

                return Err(SourceError::NoData);
            }
            Ok(Some((
                mat,
                opt_timestamp,
                Some((c, events_before, events_after, img_start_ts, img_end_ts)),
            ))) => {
                self.control_latency(opt_timestamp);

                self.input_frame_scaled = mat;
                self.dvs_c = c;
                self.dvs_events_before = Some(events_before);
                self.dvs_events_after = Some(events_after);
                self.start_of_frame_timestamp = Some(img_start_ts);
                self.end_of_frame_timestamp = Some(img_end_ts);
                self.video.ref_time_divisor =
                    (img_end_ts - img_start_ts) as f64 / f64::from(self.video.ref_time);
            }
            Ok(Some((mat, opt_timestamp, None))) => {
                self.control_latency(opt_timestamp);
                self.input_frame_scaled = mat;
            }
            Err(e) => return Err(SourceError::EdiError(e)),
        }
        let start_of_frame_timestamp = match self.start_of_frame_timestamp {
            Some(t) => t,
            None => return Err(SourceError::UninitializedData),
        };
        let end_of_frame_timestamp = match self.end_of_frame_timestamp {
            Some(t) => t,
            None => return Err(SourceError::UninitializedData),
        };
        if with_events {
            if self.video.in_interval_count == 0 {
                self.dvs_last_timestamps.par_map_inplace(|ts| {
                    *ts = start_of_frame_timestamp;
                });
            } else {
                let dvs_events_before = match &self.dvs_events_before {
                    Some(events) => events.clone(),
                    None => return Err(SourceError::UninitializedData),
                };
                self.integrate_dvs_events(
                    &dvs_events_before,
                    &start_of_frame_timestamp,
                    check_dvs_before,
                )?;
                self.integrate_frame_gaps()?;
            }
        }

        if self.input_frame_scaled.empty() {
            eprintln!("End of video");
            return Err(BufferEmpty);
        }

        match self
            .input_frame_scaled
            .convert_to(&mut self.image_8u, CV_8U, 255.0, 0.0)
        {
            Ok(_) => {}
            Err(e) => {
                return Err(SourceError::OpencvError(e));
            }
        }

        // While `input_frame_scaled` may not be continuous (which would cause problems with
        // iterating over the pixels), cloning it ensures that it is made continuous.
        // https://stackoverflow.com/questions/33665241/is-opencv-matrix-data-guaranteed-to-be-continuous
        let mut tmp = self.image_8u.clone();
        let mat_integration_time = match self.mode {
            TranscoderMode::Framed => self.video.ref_time as f32,
            TranscoderMode::RawDavis => (end_of_frame_timestamp - start_of_frame_timestamp) as f32,
            TranscoderMode::RawDvs => {
                // TODO: Note how c is fixed here, since we don't have a mechanism for determining
                // its value
                self.dvs_c = 0.15;
                match tmp.data_bytes_mut() {
                    Ok(bytes) => {
                        for byte in bytes {
                            *byte = 0;
                        }
                    }
                    Err(e) => {
                        return Err(SourceError::OpencvError(e));
                    }
                }
                0.0
            }
        };

        let ret = thread_pool.install(|| {
            self.video
                .integrate_matrix(tmp, mat_integration_time, Continuous, view_interval)
        });

        #[allow(clippy::cast_possible_wrap, clippy::cast_possible_truncation)]
        unsafe {
            for (idx, val) in self.dvs_last_ln_val.iter_mut().enumerate() {
                let px = match self.input_frame_scaled.at_unchecked::<f64>(idx as i32) {
                    Ok(px) => px,
                    Err(e) => {
                        return Err(SourceError::OpencvError(e));
                    }
                };
                match self.mode {
                    TranscoderMode::RawDavis | TranscoderMode::Framed => {
                        *val = px.ln_1p();
                    }
                    TranscoderMode::RawDvs => {
                        *val = 0.5_f64.ln_1p();
                    }
                }
            }
        }

        if with_events {
            let dvs_events_after = match &self.dvs_events_after {
                Some(events) => events.clone(),
                None => return Err(SourceError::UninitializedData),
            };
            self.dvs_last_timestamps.par_map_inplace(|ts| {
                *ts = end_of_frame_timestamp;
            });

            self.integrate_dvs_events(&dvs_events_after, &end_of_frame_timestamp, check_dvs_after)?;
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

/// Get the next APS image from the video source.
/// Returns a tuple of the image, the timestamp of the image, the timestamp of the end of the
/// frame, and the events occurring during the interval.
/// # Arguments
/// * `with_events` - Whether to return events along with the image
/// * `thread_pool` - The thread pool to use for parallelization
/// # Errors
/// * `ReconstructionError` - Some error in `davis-edi-rs`
pub async fn get_next_image(
    reconstructor: &mut Reconstructor,
    thread_pool: &ThreadPool,
    with_events: bool,
) -> Result<Option<IterVal>, ReconstructionError> {
    thread_pool
        .install(|| async {
            match reconstructor.next(with_events).await {
                None => {
                    println!("\nFinished!");
                    Ok(None)
                }
                Some(res) => match res {
                    Ok(a) => Ok(Some(a)),
                    Err(e) => Err(e),
                },
            }
        })
        .await
}
