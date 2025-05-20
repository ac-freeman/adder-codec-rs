use crate::transcoder::source::video::FramedViewMode::SAE;
use crate::transcoder::source::video::SourceError::BufferEmpty;
use crate::transcoder::source::video::{
    integrate_for_px, Source, SourceError, Video, VideoBuilder,
};
use adder_codec_core::Mode::{Continuous, FramePerfect};
use adder_codec_core::{DeltaT, PixelMultiMode};
use davis_edi_rs::aedat::events_generated::Event as DvsEvent;
use davis_edi_rs::util::reconstructor::{IterVal, ReconstructionError, Reconstructor};
use rayon::iter::IndexedParallelIterator;
use rayon::iter::ParallelIterator;

use opencv::core::{Mat, CV_8U};
use opencv::prelude::*;

use bumpalo::Bump;
use ndarray::{Array3, Axis};

use rayon::iter::IntoParallelIterator;
use rayon::{current_num_threads, ThreadPool};
use std::cmp::max;
use std::error::Error;
use std::io::Write;
use std::mem::swap;
use std::thread;

use adder_codec_core::codec::{CodecError, EncoderOptions, EncoderType};
use adder_codec_core::{Event, PlaneSize, SourceCamera, SourceType, TimeMode};

use crate::framer::scale_intensity::{FrameValue, SaeTime};
use crate::transcoder::event_pixel_tree::Intensity32;
use crate::utils::cv::clamp_u8;
use crate::utils::viz::ShowFeatureMode;
use tokio::runtime::Runtime;
use video_rs_adder_dep::Frame;

/// The EDI reconstruction mode, determining how intensities are integrated for the ADΔER model
#[derive(PartialEq, Eq, Clone, Copy, Debug)]
pub enum TranscoderMode {
    /// Perform a framed EDI reconstruction at a given (constant) frame rate. Each frame is
    /// integrated in the ADΔER model with a [Framed](crate::transcoder::source::framed::Framed) source.
    Framed,

    /// Use EDI to reconstruct only one intensity frame for each input APS frame. That is, each
    /// APS frame is deblurred, by using the DVS events that occur during that exposure.
    /// The DVS events between deblurred APS frames are integrated directly and asynchronously
    /// into the ADΔER model.
    RawDavis,

    /// Use EDI merely as a driver for providing the DVS events. The DVS events between are
    /// integrated directly and asynchronously into the ADΔER model. Any APS frames are ignored.
    RawDvs,
}

struct Integration<W> {
    dvs_c: f64,
    dvs_events_before: Option<Vec<DvsEvent>>,
    dvs_events_last_after: Option<Vec<DvsEvent>>,
    dvs_events_after: Option<Vec<DvsEvent>>,

    pub temp_first_frame_start_timestamp: i64,

    /// The timestamp for the start of the APS frame exposure
    pub start_of_frame_timestamp: Option<i64>,

    /// The timestamp for the end of the APS frame exposure
    pub end_of_frame_timestamp: Option<i64>,

    pub end_of_last_frame_timestamp: Option<i64>,

    /// The timestamp of the last DVS event integrated for each pixel
    pub dvs_last_timestamps: Array3<i64>,

    /// The log-space last intensity value for each pixel
    pub dvs_last_ln_val: Array3<f64>,

    phantom: std::marker::PhantomData<W>,
}

/// Attributes of a framed video -> ADΔER transcode
pub struct Davis<W: Write + std::marker::Send + std::marker::Sync + 'static> {
    reconstructor: Option<Reconstructor>,
    pub(crate) input_frame_scaled: Mat,
    pub(crate) video: Video<W>,
    image_8u: Mat,
    thread_pool_edi: Option<ThreadPool>,

    integration: Integration<W>,

    /// The latency between a DAVIS/DVS packet being sent by the camera and read by the reconstructor
    latency: u128,

    cached_mat_opt: Option<Option<IterVal>>,

    optimize_adder_controller: bool,

    /// The EDI reconstruction mode, determining how intensities are integrated for the ADΔER model
    pub mode: TranscoderMode,

    time_change: f64,
    num_dvs_events: usize,
    ref_time_divisor: f64,
}

unsafe impl<W: Write + std::marker::Send + std::marker::Sync> Sync for Davis<W> {}

impl<W: Write + 'static + std::marker::Send + std::marker::Sync> Davis<W> {
    /// Create a new `Davis` transcoder
    pub fn new(reconstructor: Reconstructor, mode: TranscoderMode) -> Result<Self, Box<dyn Error>> {
        let plane = PlaneSize::new(reconstructor.width, reconstructor.height, 1)?;

        let video = Video::new(
            plane,
            match mode {
                TranscoderMode::Framed => FramePerfect,
                TranscoderMode::RawDavis => Continuous,
                TranscoderMode::RawDvs => Continuous,
            },
            None,
        )?
        .chunk_rows(plane.h_usize() / 4);
        let thread_pool_edi = rayon::ThreadPoolBuilder::new()
            .num_threads(max(current_num_threads() - 4, 1))
            .build()?;

        let plane = &video.state.plane;

        let timestamps = vec![0_i64; video.state.plane.volume()];

        let dvs_last_timestamps: Array3<i64> = Array3::from_shape_vec(
            (plane.h().into(), plane.w().into(), plane.c().into()),
            timestamps,
        )?;

        let timestamps = vec![0.0_f64; video.state.plane.volume()];

        let dvs_last_ln_val: Array3<f64> = Array3::from_shape_vec(
            (plane.h() as usize, plane.w() as usize, plane.c() as usize),
            timestamps,
        )?;

        let davis_source = Davis {
            reconstructor: Some(reconstructor),
            input_frame_scaled: Mat::default(),
            video,
            image_8u: Mat::default(),
            thread_pool_edi: Some(thread_pool_edi),

            integration: Integration {
                dvs_c: 0.15,
                dvs_events_before: None,
                dvs_events_after: None,
                dvs_events_last_after: None,
                temp_first_frame_start_timestamp: 0,
                start_of_frame_timestamp: None,
                end_of_frame_timestamp: None,
                end_of_last_frame_timestamp: None,
                dvs_last_timestamps,
                dvs_last_ln_val,
                phantom: std::marker::PhantomData,
            },
            latency: 0,
            cached_mat_opt: None,

            optimize_adder_controller: false,
            mode: TranscoderMode::Framed,
            time_change: 0.0,
            num_dvs_events: 0,
            ref_time_divisor: 1.0,
        };

        Ok(davis_source)
    }

    /// Set whether to optimize the EDI controller (default: `false`) during EDI reconstruction.
    ///
    /// If true, then the program will regularly re-calculate the optimal DVS contrast threshold.
    pub fn optimize_adder_controller(mut self, optimize: bool) -> Self {
        self.optimize_adder_controller = optimize;
        self
    }

    /// Set the [`TranscoderMode`] (default: [`TranscoderMode::Framed`])
    pub fn mode(mut self, mode: TranscoderMode) -> Self {
        self.mode = mode;
        self
    }

    // #[allow(clippy::cast_precision_loss)]
    // fn control_latency(&mut self, opt_timestamp: Option<Instant>) {
    //     if self.optimize_adder_controller {
    //         match opt_timestamp {
    //             None => {}
    //             Some(timestamp) => {
    //                 let latency = timestamp.elapsed().as_millis();
    //                 if latency as f64 >= self.reconstructor.target_latency * 3.0 {
    //                     self.video.state.c_thresh_pos =
    //                         self.video.state.c_thresh_pos.saturating_add(1);
    //                     self.video.state.c_thresh_neg =
    //                         self.video.state.c_thresh_neg.saturating_add(1);
    //                 } else {
    //                     self.video.state.c_thresh_pos =
    //                         self.video.state.c_thresh_pos.saturating_sub(1);
    //                     self.video.state.c_thresh_neg =
    //                         self.video.state.c_thresh_neg.saturating_sub(1);
    //                 }
    //                 eprintln!(
    //                     "    adder latency = {}, adder c = {}",
    //                     latency, self.video.state.c_thresh_pos
    //                 );
    //             }
    //         }
    //     }
    // }

    /// Get an immutable reference to the [`Reconstructor`]
    pub fn get_reconstructor(&self) -> &Option<Reconstructor> {
        &self.reconstructor
    }

    /// Get a mutable reference to the [`Reconstructor`]
    pub fn get_reconstructor_mut(&mut self) -> &mut Option<Reconstructor> {
        &mut self.reconstructor
    }

    /// Get the latency of the EDI controller, in milliseconds
    pub fn get_latency(&self) -> u128 {
        self.latency
    }
}

impl<W: Write + 'static + std::marker::Send + std::marker::Sync> Integration<W> {
    /// Integrate a sequence of [DVS events](DvsEvent) into the ADΔER video model
    #[allow(clippy::cast_sign_loss)]
    pub fn integrate_dvs_events<
        F: Fn(i64, i64) -> bool + Send + 'static + std::marker::Sync,
        G: Fn(i64, i64) -> bool + Send + 'static + std::marker::Sync,
    >(
        &mut self,
        video: &mut Video<W>,
        dvs_events: &Vec<DvsEvent>,
        frame_timestamp: i64,
        event_check_1: F,
        frame_timestamp_2: Option<i64>,
        event_check_2: G,
    ) -> Result<(), CodecError> {
        // TODO: not fixed 4 chunks?
        let mut dvs_chunks: [Vec<DvsEvent>; 4] = [
            Vec::with_capacity(100_000),
            Vec::with_capacity(100_000),
            Vec::with_capacity(100_000),
            Vec::with_capacity(100_000),
        ];

        let mut chunk_idx;
        for dvs_event in dvs_events {
            chunk_idx = dvs_event.y() as usize / (video.state.plane.h_usize() / 4);
            dvs_chunks[chunk_idx].push(*dvs_event);
        }

        let chunk_rows = video.state.chunk_rows;
        // let px_per_chunk: usize =
        //     self.video.chunk_rows * self.video.width as usize * self.video.channels as usize;
        let big_buffer: Vec<Vec<Event>> = video
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
                        if event_check_1(event.t(), frame_timestamp)
                            && if let Some(frame_timestamp_2) = frame_timestamp_2 {
                                event_check_2(event.t(), frame_timestamp_2)
                            } else {
                                true
                            }
                        {
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
                            if delta_t_micro == event.t() {
                                continue;
                            }
                            let ticks_per_micro = video.state.tps as f32 / 1e6;
                            let delta_t_ticks = delta_t_micro as f32 * ticks_per_micro;
                            if delta_t_ticks < 0.0 {
                                // Should get here only if the event has already been processed?
                                continue; // TODO: do better
                            }

                            // First, integrate the previous value enough to fill the time since then
                            let first_integration = ((last_val as Intensity32)
                                / video.state.params.ref_time as f32
                                * delta_t_ticks)
                                .max(0.0);

                            if px.need_to_pop_top {
                                buffer.push(px.pop_top_event(
                                    first_integration,
                                    Continuous,
                                    video.state.params.ref_time,
                                ));
                            }

                            let running_t_before = px.running_t;
                            px.integrate(
                                first_integration,
                                delta_t_ticks,
                                Continuous,
                                video.state.params.delta_t_max,
                                video.state.params.ref_time,
                                video.encoder.options.crf.get_parameters().c_thresh_max,
                                video
                                    .encoder
                                    .options
                                    .crf
                                    .get_parameters()
                                    .c_increase_velocity,
                                video.state.params.pixel_multi_mode,
                            );
                            let running_t_after = px.running_t;
                            debug_assert_eq!(running_t_after, running_t_before + delta_t_ticks);

                            if px.need_to_pop_top {
                                buffer.push(px.pop_top_event(
                                    first_integration,
                                    Continuous,
                                    video.state.params.ref_time,
                                ));
                            }
                            let running_t_after = px.running_t;
                            debug_assert_eq!(running_t_after, running_t_before + delta_t_ticks);

                            ///////////////////////////////////////////////////////
                            // Then, integrate a tiny amount of the next intensity
                            // let mut frame_val = (base_val as f64);
                            // let mut lat_frame_val = (frame_val / 255.0).ln();

                            *last_val_ln *= if event.on() { self.dvs_c } else { -self.dvs_c }.exp();
                            let mut frame_val = (last_val_ln.exp() - 1.0) * 255.0;
                            clamp_u8(&mut frame_val, last_val_ln);

                            let frame_val_u8 = frame_val as u8; // TODO: don't let this be lossy here

                            if frame_val_u8 < base_val.saturating_sub(px.c_thresh)
                                || frame_val_u8 > base_val.saturating_add(px.c_thresh)
                            {
                                px.pop_best_events(
                                    &mut buffer,
                                    Continuous,
                                    video.state.params.pixel_multi_mode,
                                    video.state.params.ref_time,
                                    frame_val as Intensity32,
                                );
                                px.base_val = frame_val_u8;

                                // If continuous mode and the D value needs to be different now
                                match px.set_d_for_continuous(
                                    frame_val as Intensity32,
                                    video.state.params.ref_time,
                                ) {
                                    None => {}
                                    Some(event) => buffer.push(event),
                                };
                            }
                            let tmpp = dvs_last_timestamps_chunk
                                [[event.y() as usize % chunk_rows, event.x() as usize, 0]];

                            dvs_last_timestamps_chunk
                                [[event.y() as usize % chunk_rows, event.x() as usize, 0]] =
                                event.t();

                            debug_assert!(
                                dvs_last_timestamps_chunk
                                    [[event.y() as usize % chunk_rows, event.x() as usize, 0]]
                                    >= tmpp
                            );
                        }
                    }

                    buffer
                },
            )
            .collect();

        let db: &mut [u8] = match video.display_frame_features.as_slice_mut() {
            Some(db) => db,
            None => return Err(CodecError::MalformedEncoder), // TODO: Wrong type of error
        };

        // TODO: split off into separate function
        // TODO: When there's full support for various bit-depth sources, modify this accordingly
        let practical_d_max = fast_math::log2_raw(
            255.0 * (video.state.params.delta_t_max / video.state.params.ref_time) as f32,
        );
        db.iter_mut()
            .zip(video.state.running_intensities.iter_mut())
            .enumerate()
            .for_each(|(idx, (val, running))| {
                let y = idx / video.state.plane.area_wc();
                let x = (idx % video.state.plane.area_wc()) / video.state.plane.c_usize();
                let c = idx % video.state.plane.c_usize();
                *val = match video.event_pixel_trees[[y, x, c]].arena[0].best_event {
                    Some(event) => u8::get_frame_value(
                        &event.into(),
                        SourceType::U8,
                        video.state.params.ref_time as f64,
                        practical_d_max,
                        video.state.params.delta_t_max,
                        video.instantaneous_view_mode,
                        if video.instantaneous_view_mode == SAE {
                            Some(SaeTime {
                                running_t: video.event_pixel_trees[[y, x, c]].running_t as DeltaT,
                                last_fired_t: video.event_pixel_trees[[y, x, c]].last_fired_t
                                    as DeltaT,
                            })
                        } else {
                            None
                        },
                    ),
                    None => *val,
                };
                *running = *val;
            });

        for events in &big_buffer {
            for e1 in events.iter() {
                video.encoder.ingest_event(*e1)?;
            }
        }

        if let Err(e) = video.handle_features(&big_buffer) {
            return Err(CodecError::VisionError(e.to_string()));
        }

        Ok(())
    }

    #[allow(clippy::cast_possible_truncation)]
    fn integrate_frame_gaps(&mut self, video: &mut Video<W>) -> Result<(), SourceError> {
        let px_per_chunk: usize = video.state.chunk_rows * video.state.plane.area_wc();

        let start_of_frame_timestamp = match self.start_of_frame_timestamp {
            Some(t) => t,
            None => return Err(SourceError::UninitializedData),
        };

        // Important: if framing the events simultaneously, then the chunk division must be
        // exactly the same as it is for the framer
        let big_buffer: Vec<Vec<Event>> = video
            .event_pixel_trees
            .axis_chunks_iter_mut(Axis(0), video.state.chunk_rows)
            .into_par_iter()
            .zip(
                self.dvs_last_ln_val
                    .axis_chunks_iter_mut(Axis(0), video.state.chunk_rows)
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

                    let mut last_val = (last_val_ln.exp() - 1.0) * 255.0;

                    clamp_u8(&mut last_val, last_val_ln);
                    *base_val = px.base_val;
                    *frame_val = last_val as u8;

                    let ticks_per_micro = video.state.tps as f32 / 1e6;

                    let delta_t_micro = start_of_frame_timestamp
                        - self.dvs_last_timestamps[[px.coord.y as usize, px.coord.x as usize, 0]];

                    if delta_t_micro == start_of_frame_timestamp {
                        continue;
                    }

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

                    let integration = ((last_val / f64::from(video.state.params.ref_time))
                        * f64::from(delta_t_ticks))
                    .max(0.0);
                    assert!(integration >= 0.0);

                    integrate_for_px(
                        px,
                        base_val,
                        *frame_val,
                        integration as f32,
                        delta_t_ticks,
                        &mut buffer,
                        &video.state.params,
                        video.encoder.options.crf.get_parameters(),
                        false,
                    );
                }
                buffer
            })
            .collect();

        let db = match video.display_frame_features.as_slice_mut() {
            Some(db) => db,
            None => {
                return Err(SourceError::VisionError(
                    "No instantaneous frame".to_string(),
                ))
            }
        };

        // TODO: split off into separate function
        // TODO: When there's full support for various bit-depth sources, modify this accordingly
        let practical_d_max = fast_math::log2_raw(
            255.0 * (video.state.params.delta_t_max / video.state.params.ref_time) as f32,
        );
        db.iter_mut()
            .zip(video.state.running_intensities.iter_mut())
            .enumerate()
            .for_each(|(idx, (val, running))| {
                let y = idx / video.state.plane.area_wc();
                let x = (idx % video.state.plane.area_wc()) / video.state.plane.c_usize();
                let c = idx % video.state.plane.c_usize();
                *val = match video.event_pixel_trees[[y, x, c]].arena[0].best_event {
                    Some(event) => u8::get_frame_value(
                        &event.into(),
                        SourceType::U8,
                        video.state.params.ref_time as f64,
                        practical_d_max,
                        video.state.params.delta_t_max,
                        video.instantaneous_view_mode,
                        if video.instantaneous_view_mode == SAE {
                            Some(SaeTime {
                                running_t: video.event_pixel_trees[[y, x, c]].running_t as DeltaT,
                                last_fired_t: video.event_pixel_trees[[y, x, c]].last_fired_t
                                    as DeltaT,
                            })
                        } else {
                            None
                        },
                    ),
                    None => *val,
                };
                *running = *val;
            });

        for events in &big_buffer {
            for e1 in events.iter() {
                video.encoder.ingest_event(*e1)?;
            }
        }

        video.handle_features(&big_buffer)?;

        Ok(())
    }
}

impl<W: Write + 'static + std::marker::Send + std::marker::Sync> Source<W> for Davis<W> {
    fn consume(&mut self) -> Result<Vec<Vec<Event>>, SourceError> {
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

        if self.reconstructor.is_none() {
            return Err(SourceError::NoData);
        }

        let mut reconstructor_holder = None;
        swap(&mut self.reconstructor, &mut reconstructor_holder);
        let mut thread_pool_holder = None;
        swap(&mut self.thread_pool_edi, &mut thread_pool_holder);
        let mat_opt_handle = thread::spawn(move || {
            get_next_image(
                reconstructor_holder.unwrap(),
                thread_pool_holder.unwrap(),
                with_events,
            )
        });

        let mut ret = Ok(vec![]);

        if self.cached_mat_opt.is_some() {
            let mut cached_mat_opt = None;
            std::mem::swap(&mut cached_mat_opt, &mut self.cached_mat_opt);

            match cached_mat_opt.unwrap() {
                None => {
                    // We've reached the end of the input. Forcibly pop the last event from each pixel.
                    println!("Popping remaining events");
                    let px_per_chunk: usize =
                        self.video.state.chunk_rows * self.video.state.plane.area_wc();
                    let big_buffer: Vec<Vec<Event>> = self
                        .video
                        .event_pixel_trees
                        .axis_chunks_iter_mut(Axis(0), self.video.state.chunk_rows)
                        .into_par_iter()
                        .enumerate()
                        .map(|(_chunk_idx, mut chunk)| {
                            let mut buffer: Vec<Event> = Vec::with_capacity(px_per_chunk);
                            for (_, px) in chunk.iter_mut().enumerate() {
                                px.pop_best_events(
                                    &mut buffer,
                                    self.video.state.params.pixel_tree_mode,
                                    self.video.state.params.pixel_multi_mode,
                                    self.video.state.params.ref_time,
                                    0.0,
                                );
                            }
                            buffer
                        })
                        .collect();

                    self.video.encoder.ingest_events_events(&big_buffer)?;

                    return Err(SourceError::NoData);
                }
                Some((
                    mat,
                    _opt_timestamp,
                    Some((c, events_before, events_after, img_start_ts, img_end_ts)),
                    opt_latency,
                )) => {
                    // We get here if we're in raw mode (getting raw events from EDI, and also
                    // potentially deblurred frames)
                    // self.control_latency(opt_timestamp);

                    self.input_frame_scaled = mat;

                    self.time_change = (img_start_ts
                        - self.integration.start_of_frame_timestamp.unwrap_or(0))
                        as f64;
                    self.num_dvs_events = events_before.len() + events_after.len();

                    self.integration.start_of_frame_timestamp = Some(img_start_ts);
                    self.integration.end_of_frame_timestamp = Some(img_end_ts);
                    if self.mode == TranscoderMode::RawDvs {
                        // assert!(events_before.is_empty());
                        self.integration.end_of_frame_timestamp = Some(img_start_ts + 1);
                    }
                    self.integration.dvs_c = c;
                    self.integration.dvs_events_before = Some(events_before);
                    self.integration.dvs_events_after = Some(events_after);

                    self.ref_time_divisor = (img_end_ts - img_start_ts) as f64
                        / self.video.state.params.ref_time as f64;
                    if let Some(latency) = opt_latency {
                        self.latency = latency;
                    }
                }
                Some((mat, _, None, opt_latency)) => {
                    // We get here if we're in framed mode (just getting deblurred frames from EDI,
                    // including intermediate frames)
                    // self.control_latency(opt_timestamp);
                    self.input_frame_scaled = mat;
                    if let Some(latency) = opt_latency {
                        self.latency = latency;
                    }
                } // Err(e) => return Err(SourceError::EdiError(e)),
            }

            let start_of_frame_timestamp = self.integration.start_of_frame_timestamp.unwrap_or(0);
            let end_of_frame_timestamp = self
                .integration
                .end_of_frame_timestamp
                .unwrap_or(self.video.state.params.ref_time.into());
            if self.integration.temp_first_frame_start_timestamp == 0 {
                self.integration.temp_first_frame_start_timestamp =
                    self.integration.start_of_frame_timestamp.unwrap_or(0);
            }
            if with_events {
                if self.video.state.in_interval_count == 0 {
                    /* If at the very beginning of the video, then we need to initialize the
                    last timestamps */
                    self.integration.dvs_last_timestamps.par_map_inplace(|ts| {
                        *ts = start_of_frame_timestamp;
                    });
                } else {
                    let dvs_events_before = match &self.integration.dvs_events_before {
                        Some(events) => events.clone(),
                        None => return Err(SourceError::UninitializedData),
                    };

                    if let (Some(events), Some(end_of_last_timestamp)) = (
                        self.integration.dvs_events_last_after.clone(),
                        self.integration.end_of_last_frame_timestamp,
                    ) {
                        self.integration.integrate_dvs_events(
                            &mut self.video,
                            &events,
                            start_of_frame_timestamp,
                            check_dvs_before,
                            if self.mode == TranscoderMode::RawDvs {
                                None
                            } else {
                                Some(end_of_last_timestamp)
                            },
                            check_dvs_after,
                        )?;
                    }

                    self.integration.integrate_dvs_events(
                        &mut self.video,
                        &dvs_events_before,
                        start_of_frame_timestamp,
                        check_dvs_before,
                        None,
                        check_dvs_before,
                    )?;

                    for px in &self.video.event_pixel_trees {
                        let a = px.running_t as i64;
                        let b = start_of_frame_timestamp
                            - self.integration.temp_first_frame_start_timestamp;
                        // debug_assert!(a <= b);
                        // debug_assert!(a <= start_of_frame_timestamp);
                    }

                    self.integration.integrate_frame_gaps(&mut self.video)?;
                    for px in &self.video.event_pixel_trees {
                        let a = px.running_t as i64;
                        let b = start_of_frame_timestamp
                            - self.integration.temp_first_frame_start_timestamp;
                        // debug_assert!(a <= b);
                        // debug_assert!(a > b - 1000);
                    }
                }
            }

            if self.input_frame_scaled.empty() {
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
                TranscoderMode::Framed => self.video.state.params.ref_time as f32,
                TranscoderMode::RawDavis => {
                    (end_of_frame_timestamp - start_of_frame_timestamp) as f32
                }
                TranscoderMode::RawDvs => {
                    // TODO: Note how c is fixed here, since we don't have a mechanism for determining
                    // its value
                    self.integration.dvs_c = 0.15;
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

            let frame = unsafe {
                video_rs_adder_dep::Frame::from_shape_vec_unchecked(
                    (
                        self.video.state.plane.h_usize(),
                        self.video.state.plane.w_usize(),
                        self.video.state.plane.c_usize(),
                    ),
                    tmp.data_bytes_mut().unwrap().to_vec(),
                )
            };

            ret = self.video.integrate_matrix(frame, mat_integration_time);

            #[allow(clippy::cast_possible_wrap, clippy::cast_possible_truncation)]
            for (idx, val) in self.integration.dvs_last_ln_val.iter_mut().enumerate() {
                let px = match
                // SAFETY:
                // `dvs_last_ln_val` is the same size as `input_frame_scaled`
                unsafe {
                    self.input_frame_scaled.at_unchecked::<f64>(idx as i32)
                } {
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

            if with_events {
                // let dvs_events_after = match &self.integration.dvs_events_after {
                //     Some(events) => events.clone(),
                //     None => return Err(SourceError::UninitializedData),
                // };
                self.integration.dvs_events_last_after = self.integration.dvs_events_after.clone();
                self.integration.end_of_last_frame_timestamp =
                    self.integration.end_of_frame_timestamp;

                self.integration.dvs_last_timestamps.par_map_inplace(|ts| {
                    debug_assert!(*ts < end_of_frame_timestamp);
                    *ts = end_of_frame_timestamp;
                });

                // for px in &self.video.event_pixel_trees {
                //     let a = px.running_t as i64;
                //     let b = self.integration.dvs_last_timestamps
                //         [[px.coord.y as usize, px.coord.x as usize, 0]]
                //         - self.integration.temp_first_frame_start_timestamp;
                //     assert_eq!(a, b);
                // }
            }
        }

        // self.cached_mat_opt = Some(
        match mat_opt_handle.join().unwrap() {
            Ok(mat_opt) => {
                self.reconstructor = Some(mat_opt.0);
                self.thread_pool_edi = Some(mat_opt.1);
                self.cached_mat_opt = Some(mat_opt.2);
            }
            Err(e) => {
                return Err(SourceError::EdiError(e));
            }
        }

        ret
    }

    fn crf(&mut self, crf: u8) {
        self.video.update_crf(crf);
    }

    fn get_video_mut(&mut self) -> &mut Video<W> {
        &mut self.video
    }

    fn get_video_ref(&self) -> &Video<W> {
        &self.video
    }

    fn get_video(self) -> Video<W> {
        self.video
    }

    fn get_input(&self) -> Option<&Frame> {
        None
    }

    fn get_running_input_bitrate(&self) -> f64 {
        match self.mode {
            TranscoderMode::Framed => {
                let fps = self.video.state.tps as f64
                    / (self.ref_time_divisor * self.video.state.params.ref_time as f64);
                self.video.state.plane.volume() as f64 * 8.0 * fps
            }
            TranscoderMode::RawDavis => {
                // dvs events per second
                let time_mult = 1E6 / self.time_change;
                let events_per_sec = self.num_dvs_events as f64 * time_mult;
                let event_bits_per_sec = events_per_sec * 9.0 * 8.0; // Best-case 9 bytes per raw DVS event

                let frame_bits_per_sec = time_mult * self.video.state.plane.volume() as f64 * 8.0;
                event_bits_per_sec + frame_bits_per_sec
            }
            TranscoderMode::RawDvs => {
                let time_mult = 1E6 / self.time_change;
                let events_per_sec = self.num_dvs_events as f64 * time_mult;
                // Best-case 9 bytes per raw DVS event
                events_per_sec * 9.0 * 8.0
            }
        }
    }
}

impl<W: Write + 'static + std::marker::Send + std::marker::Sync> VideoBuilder<W> for Davis<W> {
    fn crf(mut self, crf: u8) -> Self {
        self.video.update_crf(crf);
        self
    }

    fn quality_manual(
        mut self,
        c_thresh_baseline: u8,
        c_thresh_max: u8,
        delta_t_max_multiplier: u32,
        c_increase_velocity: u8,
        feature_c_radius_denom: f32,
    ) -> Self {
        self.video.update_quality_manual(
            c_thresh_baseline,
            c_thresh_max,
            delta_t_max_multiplier,
            c_increase_velocity,
            feature_c_radius_denom,
        );
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
        eprintln!("setting dtref to {}", ref_time);
        self.video = self
            .video
            .time_parameters(tps, ref_time, delta_t_max, time_mode)?;
        Ok(self)
    }

    fn write_out(
        mut self,
        source_camera: SourceCamera,
        time_mode: TimeMode,
        pixel_multi_mode: PixelMultiMode,
        adu_interval: Option<usize>,
        encoder_type: EncoderType,
        encoder_options: EncoderOptions,
        write: W,
    ) -> Result<Box<Self>, SourceError> {
        self.video = self.video.write_out(
            Some(source_camera),
            Some(time_mode),
            Some(pixel_multi_mode),
            adu_interval,
            encoder_type,
            encoder_options,
            write,
        )?;
        Ok(Box::new(self))
    }

    fn detect_features(mut self, detect_features: bool, show_features: ShowFeatureMode) -> Self {
        self.video = self.video.detect_features(detect_features, show_features);
        self
    }

    #[cfg(feature = "feature-logging")]
    fn log_path(self, _name: String) -> Self {
        todo!()
    }
}

fn check_dvs_before(dvs_event_t: i64, timestamp_before: i64) -> bool {
    dvs_event_t < timestamp_before
}

fn check_dvs_after(dvs_event_t: i64, timestamp_after: i64) -> bool {
    dvs_event_t > timestamp_after
}

/// Get the next APS image from the video source.
/// Returns a tuple of the image, the timestamp of the image, the timestamp of the end of the
/// frame, and the events occurring during the interval.
/// # Arguments
/// * `with_events` - Whether to return events along with the image
/// * `thread_pool` - The thread pool to use for parallelization
/// # Errors
/// * `ReconstructionError` - Some error in `davis-edi-rs`
pub fn get_next_image(
    mut reconstructor: Reconstructor,
    thread_pool: ThreadPool,
    with_events: bool,
) -> Result<(Reconstructor, ThreadPool, Option<IterVal>), ReconstructionError> {
    let res = thread_pool.install(|| async {
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
    });
    let res = futures::executor::block_on(res);
    match res {
        Ok(a) => Ok((reconstructor, thread_pool, a)),
        Err(e) => Err(e),
    }
}
