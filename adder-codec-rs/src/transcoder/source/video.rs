use chrono::prelude::*;

#[cfg(feature = "open-cv")]
use opencv::core::{KeyPoint, Mat, Scalar, Size, Vector, CV_32F, CV_32FC3, CV_8U, CV_8UC3};
use raw_parts::RawParts;
use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::io::{sink, Write};
use std::mem::swap;
use std::os::raw::c_void;

use adder_codec_core::codec::empty::stream::EmptyOutput;
use adder_codec_core::codec::encoder::Encoder;
use adder_codec_core::codec::raw::stream::RawOutput;
use adder_codec_core::codec::{
    CodecError, CodecMetadata, EncoderOptions, EncoderType, LATEST_CODEC_VERSION,
};
use adder_codec_core::{
    Coord, DeltaT, Event, Mode, PixelAddress, PlaneError, PlaneSize, SourceCamera, SourceType,
    TimeMode,
};
use bumpalo::Bump;
use std::sync::mpsc::{channel, Sender};
use std::time::{Duration, Instant, SystemTime};

use crate::framer::scale_intensity::{FrameValue, SaeTime};
use crate::transcoder::event_pixel_tree::{Intensity32, PixelArena};
use adder_codec_core::D;
#[cfg(feature = "opencv")]
use davis_edi_rs::util::reconstructor::ReconstructionError;
#[cfg(feature = "opencv")]
use opencv::{highgui, imgproc::resize, prelude::*};

#[cfg(feature = "compression")]
use adder_codec_core::codec::compressed::stream::CompressedOutput;
use adder_codec_core::Mode::Continuous;
use itertools::Itertools;
use ndarray::{Array, Array3, Axis, ShapeError, Zip};
use rayon::iter::ParallelIterator;
use rayon::iter::{IndexedParallelIterator, IntoParallelRefMutIterator};
use rayon::iter::{IntoParallelIterator, IntoParallelRefIterator};
use rayon::ThreadPool;

use crate::transcoder::source::video::FramedViewMode::SAE;
use crate::transcoder::source::{CRF, DEFAULT_CRF_QUALITY};
use crate::utils::cv::is_feature;
#[cfg(feature = "feature-logging")]
use crate::utils::logging::{LogFeature, LogFeatureSource};
use crate::utils::viz::{draw_feature_coord, draw_feature_event, ShowFeatureMode};
use thiserror::Error;
use tokio::task::JoinError;
use video_rs::Frame;

/// Various errors that can occur during an ADΔER transcode
#[derive(Error, Debug)]
pub enum SourceError {
    /// Could not open source file
    #[error("Could not open source file")]
    Open,

    /// Incorrect parameters for the given source
    #[error("ADDER parameters are invalid for the given source: `{0}`")]
    BadParams(String),

    /// When a [Framed](crate::transcoder::source::framed::Framed) source is used, but the start frame is out of bounds"
    #[error("start frame `{0}` is out of bounds")]
    StartOutOfBounds(u32),

    /// No more data to consume from the video source
    #[error("Source buffer is empty")]
    BufferEmpty,

    /// Source buffer channel is closed
    #[error("Source buffer channel is closed")]
    BufferChannelClosed,

    /// No data from next spot in buffer
    #[error("No data from next spot in buffer")]
    NoData,

    /// Data not initialized
    #[error("Data not initialized")]
    UninitializedData,

    #[cfg(feature = "open-cv")]
    /// OpenCV error
    #[error("OpenCV error")]
    OpencvError(opencv::Error),

    /// video-rs error
    #[error("video-rs error")]
    VideoError(video_rs::Error),

    /// Codec error
    #[error("Codec core error")]
    CodecError(CodecError),

    #[cfg(feature = "open-cv")]
    /// EDI error
    #[error("EDI error")]
    EdiError(ReconstructionError),

    /// Shape error
    #[error("Shape error")]
    ShapeError(#[from] ShapeError),

    /// Plane error
    #[error("Plane error")]
    PlaneError(#[from] PlaneError),

    /// Handle join error
    #[error("Handle join error")]
    JoinError(#[from] JoinError),

    /// Vision application error
    #[error("Vision application error")]
    VisionError(String),
}

#[cfg(feature = "open-cv")]
impl From<opencv::Error> for SourceError {
    fn from(value: opencv::Error) -> Self {
        SourceError::OpencvError(value)
    }
}
impl From<adder_codec_core::codec::CodecError> for SourceError {
    fn from(value: CodecError) -> Self {
        SourceError::CodecError(value)
    }
}

impl From<video_rs::Error> for SourceError {
    fn from(value: video_rs::Error) -> Self {
        SourceError::VideoError(value)
    }
}

/// The display mode
#[derive(PartialEq, Eq, Clone, Copy, Debug, Default)]
pub enum FramedViewMode {
    /// Visualize the intensity (2^[`D`] / [`DeltaT`]) of each pixel's most recent event
    #[default]
    Intensity,

    /// Visualize the [`D`] component of each pixel's most recent event
    D,

    /// Visualize the temporal component ([`DeltaT`]) of each pixel's most recent event
    DeltaT,

    /// Surface of Active Events. Visualize the time elapsed since each pixel last fired an event
    /// (most recent events will have greater values)
    SAE,
}

/// Running state of the video transcode
#[derive(Debug)]
pub struct VideoState {
    /// The size of the imaging plane
    pub plane: PlaneSize,
    pub(crate) pixel_tree_mode: Mode,

    /// The number of rows of pixels to process at a time (per thread)
    pub chunk_rows: usize,

    /// The number of input intervals (of fixed time) processed so far
    pub in_interval_count: u32,
    // pub(crate) c_thresh_pos: u8,
    // pub(crate) c_thresh_neg: u8,
    /// The baseline (starting) contrast threshold for all pixels
    pub c_thresh_baseline: u8,

    /// The maximum contrast threshold for all pixels
    pub c_thresh_max: u8,

    /// The velocity at which to increase the contrast threshold for all pixels (increment c by 1
    /// for every X input intervals, if it's stable)
    pub c_increase_velocity: u8,

    /// The maximum time difference between events of the same pixel, in ticks
    pub delta_t_max: u32,

    /// The reference time in ticks
    pub ref_time: u32,
    pub(crate) ref_time_divisor: f64,
    pub(crate) tps: DeltaT,

    /// Constant Rate Factor (CRF) quality setting for the encoder. 0 is lossless, 9 is worst quality.
    /// Determines:
    /// * The baseline (starting) c-threshold for all pixels
    /// * The maximum c-threshold for all pixels
    /// * The Dt_max multiplier
    /// * The c-threshold increase velocity (how often to increase C if the intensity is stable)
    /// * The radius for which to reset the c-threshold for neighboring pixels (if feature detection is enabled)
    pub crf_quality: u8,
    pub(crate) show_display: bool,
    pub(crate) show_live: bool,

    /// Whether or not to detect features
    pub feature_detection: bool,

    /// The current instantaneous frame, for determining features
    pub running_intensities: Array3<u8>,

    /// Whether or not to draw the features on the display mat, and the mode to do it in
    show_features: ShowFeatureMode,

    /// The radius for which to reset the c-threshold for neighboring pixels (if feature detection is enabled)
    pub feature_c_radius: u16,

    features: Vec<HashSet<Coord>>,

    feature_log_handle: Option<std::fs::File>,
}

impl Default for VideoState {
    fn default() -> Self {
        let mut state = VideoState {
            plane: PlaneSize::default(),
            pixel_tree_mode: Continuous,
            chunk_rows: 64,
            in_interval_count: 1,
            c_thresh_baseline: 0,
            c_thresh_max: 0,
            c_increase_velocity: 1,
            delta_t_max: 7650,
            ref_time: 255,
            ref_time_divisor: 1.0,
            tps: 7650,
            crf_quality: 0,
            show_display: false,
            show_live: false,
            feature_detection: false,
            running_intensities: Default::default(),
            show_features: ShowFeatureMode::Off,
            feature_c_radius: 0,
            features: Default::default(),
            feature_log_handle: None,
        };
        state.update_crf(DEFAULT_CRF_QUALITY, false);
        state
    }
}

impl VideoState {
    fn update_crf(&mut self, crf: u8, update_time_params: bool) {
        self.crf_quality = crf;
        self.c_thresh_baseline = CRF[crf as usize][0] as u8;
        self.c_thresh_max = CRF[crf as usize][1] as u8;

        if update_time_params {
            self.delta_t_max = CRF[crf as usize][2] as u32 * self.ref_time;
        }
        self.c_increase_velocity = CRF[crf as usize][3] as u8;
        self.feature_c_radius = (CRF[crf as usize][4] * self.plane.min_resolution() as f32) as u16;
    }

    fn update_quality_manual(
        &mut self,
        c_thresh_baseline: u8,
        c_thresh_max: u8,
        delta_t_max_multiplier: u32,
        c_increase_velocity: u8,
        feature_c_radius: f32,
    ) {
        self.c_thresh_baseline = c_thresh_baseline;
        self.c_thresh_max = c_thresh_max;
        self.delta_t_max = delta_t_max_multiplier * self.ref_time;
        self.c_increase_velocity = c_increase_velocity;
        self.feature_c_radius = feature_c_radius as u16; // The absolute pixel count radius
    }
}

/// A builder for a [`Video`]
pub trait VideoBuilder<W> {
    /// Set both the positive and negative contrast thresholds
    fn contrast_thresholds(self, c_thresh_pos: u8, c_thresh_neg: u8) -> Self;

    /// Set the Constant Rate Factor (CRF) quality setting for the encoder. 0 is lossless, 9 is worst quality.
    fn crf(self, crf: u8) -> Self;

    /// Manually set the parameters dictating quality
    fn quality_manual(
        self,
        c_thresh_baseline: u8,
        c_thresh_max: u8,
        delta_t_max_multiplier: u32,
        c_increase_velocity: u8,
        feature_c_radius_denom: f32,
    ) -> Self;

    /// Set the positive contrast threshold
    #[deprecated(since = "0.3.4", note = "please use `crf` or `quality_manual` instead")]
    fn c_thresh_pos(self, c_thresh_pos: u8) -> Self;

    /// Set the negative contrast threshold
    #[deprecated(since = "0.3.4", note = "please use `crf` or `quality_manual` instead")]
    fn c_thresh_neg(self, c_thresh_neg: u8) -> Self;

    /// Set the chunk rows
    fn chunk_rows(self, chunk_rows: usize) -> Self;

    /// Set the time parameters
    fn time_parameters(
        self,
        tps: DeltaT,
        ref_time: DeltaT,
        delta_t_max: DeltaT,
        time_mode: Option<TimeMode>,
    ) -> Result<Self, SourceError>
    where
        Self: std::marker::Sized;

    /// Set the [`Encoder`]
    fn write_out(
        self,
        source_camera: SourceCamera,
        time_mode: TimeMode,
        encoder_type: EncoderType,
        encoder_options: EncoderOptions,
        write: W,
    ) -> Result<Box<Self>, SourceError>;

    /// Set whether or not the show the live display
    fn show_display(self, show_display: bool) -> Self;

    /// Set whether or not to detect features, and whether or not to display the features
    fn detect_features(self, detect_features: bool, show_features: ShowFeatureMode) -> Self;
}

// impl VideoBuilder for Video {}

/// Attributes common to ADΔER transcode process
pub struct Video<W: Write> {
    /// The current state of the video transcode
    pub state: VideoState,
    pub(crate) event_pixel_trees: Array3<PixelArena>,

    /// The current instantaneous display frame
    pub display_frame: Frame,

    /// The current view mode of the instantaneous frame
    pub instantaneous_view_mode: FramedViewMode,

    /// Channel for sending events to the encoder
    pub event_sender: Sender<Vec<Event>>,
    pub(crate) encoder: Encoder<W>,

    pub encoder_type: EncoderType,
    // TODO: Hold multiple encoder options and an enum, so that boxing isn't required.
    // Also hold a state for whether or not to write out events at all, so that a null writer isn't required.
    // Eric: this is somewhat addressed above
}
unsafe impl<W: Write> Send for Video<W> {}

impl<W: Write + 'static> Video<W> {
    /// Initialize the Video with default parameters.
    pub(crate) fn new(
        plane: PlaneSize,
        pixel_tree_mode: Mode,
        writer: Option<W>,
    ) -> Result<Video<W>, SourceError> {
        let mut state = VideoState {
            pixel_tree_mode,
            running_intensities: Array::zeros((plane.h_usize(), plane.w_usize(), plane.c_usize())),
            ..Default::default()
        };

        #[cfg(feature = "feature-logging")]
        {
            let date_time = Local::now();
            let formatted = format!("features_{}.log", date_time.format("%d_%m_%Y_%H_%M_%S"));
            let log_handle = std::fs::File::create(formatted).ok();
            state.feature_log_handle = log_handle;

            // Write the plane size to the log file
            if let Some(handle) = &mut state.feature_log_handle {
                writeln!(handle, "{}x{}x{}", plane.w(), plane.h(), plane.c()).unwrap();
            }
        }

        let mut data = Vec::new();
        for y in 0..plane.h() {
            for x in 0..plane.w() {
                for c in 0..plane.c() {
                    let px = PixelArena::new(
                        1.0,
                        Coord {
                            x,
                            y,
                            c: match &plane.c() {
                                1 => None,
                                _ => Some(c),
                            },
                        },
                    );
                    data.push(px);
                }
            }
        }

        let event_pixel_trees: Array3<PixelArena> =
            Array3::from_shape_vec((plane.h_usize(), plane.w_usize(), plane.c_usize()), data)?;
        let mut instantaneous_frame =
            Array3::zeros((plane.h_usize(), plane.w_usize(), plane.c_usize()));

        state.plane = plane;
        let instantaneous_view_mode = FramedViewMode::Intensity;
        let (event_sender, _) = channel();
        let meta = CodecMetadata {
            codec_version: LATEST_CODEC_VERSION,
            header_size: 0,
            time_mode: TimeMode::AbsoluteT,
            plane: state.plane,
            tps: state.tps,
            ref_interval: state.ref_time,
            delta_t_max: state.delta_t_max,
            event_size: 0,
            source_camera: SourceCamera::default(), // TODO: Allow for setting this
        };

        match writer {
            None => {
                let encoder: Encoder<W> = Encoder::new_empty(EmptyOutput::new(meta, sink()));
                Ok(Video {
                    state,
                    event_pixel_trees,
                    display_frame: instantaneous_frame,
                    instantaneous_view_mode,
                    event_sender,
                    encoder,
                    encoder_type: EncoderType::Empty,
                })
            }
            Some(w) => {
                let encoder = Encoder::new_raw(
                    // TODO: Allow for compressed representation (not just raw)
                    RawOutput::new(meta, w),
                    EncoderOptions::default(),
                );
                Ok(Video {
                    state,
                    event_pixel_trees,
                    display_frame: instantaneous_frame,
                    instantaneous_view_mode,
                    event_sender,
                    encoder,
                    encoder_type: EncoderType::Empty,
                })
            }
        }
    }

    /// Set the positive contrast threshold
    #[deprecated(
        since = "0.3.4",
        note = "please use `update_crf` or `update_quality_manual` instead"
    )]
    pub fn c_thresh_pos(mut self, c_thresh_pos: u8) -> Self {
        for px in self.event_pixel_trees.iter_mut() {
            px.c_thresh = c_thresh_pos;
        }
        self.state.c_thresh_baseline = c_thresh_pos;
        self
    }

    /// Set the negative contrast threshold
    #[deprecated(
        since = "0.3.4",
        note = "please use `update_crf` or `update_quality_manual` instead"
    )]
    pub fn c_thresh_neg(self, _c_thresh_neg: u8) -> Self {
        unimplemented!();
        // for px in self.event_pixel_trees.iter_mut() {
        //     px.c_thresh = c_thresh_neg;
        // }
        // self
    }

    /// Set the number of rows to process at a time (in each thread)
    pub fn chunk_rows(mut self, chunk_rows: usize) -> Self {
        self.state.chunk_rows = chunk_rows;
        let mut num_chunks = self.state.plane.h_usize() / chunk_rows;
        if self.state.plane.h_usize() % chunk_rows != 0 {
            num_chunks += 1;
        }
        self.state.features = vec![HashSet::new(); num_chunks];
        self
    }

    /// Set the time parameters for the video.
    ///
    /// These parameters, in conjunction, determine the temporal resolution and maximum transcode
    /// accuracy/quality.
    ///
    /// # Arguments
    ///
    /// * `tps`: ticks per second
    /// * `ref_time`: reference time in ticks.
    /// * `delta_t_max`: maximum time difference between events of the same pixel, in ticks
    ///
    /// returns: `Result<Video<W>, Box<dyn Error, Global>>`
    pub fn time_parameters(
        mut self,
        tps: DeltaT,
        ref_time: DeltaT,
        delta_t_max: DeltaT,
        time_mode: Option<TimeMode>,
    ) -> Result<Self, SourceError> {
        self.event_pixel_trees.par_map_inplace(|px| {
            px.time_mode(time_mode);
        });

        if ref_time > f32::MAX as u32 {
            eprintln!(
                "Reference time {} is too large. Keeping current value of {}.",
                ref_time, self.state.ref_time
            );
            return Ok(self);
        }
        if tps > f32::MAX as u32 {
            eprintln!(
                "Time per sample {} is too large. Keeping current value of {}.",
                tps, self.state.tps
            );
            return Ok(self);
        }
        if delta_t_max > f32::MAX as u32 {
            eprintln!(
                "Delta t max {} is too large. Keeping current value of {}.",
                delta_t_max, self.state.delta_t_max
            );
            return Ok(self);
        }
        if delta_t_max < ref_time {
            eprintln!(
                "Delta t max {} is smaller than reference time {}. Keeping current value of {}.",
                delta_t_max, ref_time, self.state.delta_t_max
            );
            return Ok(self);
        }
        self.state.delta_t_max = delta_t_max;
        self.state.ref_time = ref_time;
        self.state.tps = tps;

        Ok(self)
    }

    /// Write out the video to a file.
    ///
    /// # Arguments
    ///
    /// * `source_camera`: the type of video source
    /// * `time_mode`: the time mode of the video
    /// * `write`: the output stream to write to
    pub fn write_out(
        mut self,
        source_camera: Option<SourceCamera>,
        time_mode: Option<TimeMode>,
        encoder_type: EncoderType,
        encoder_options: EncoderOptions,
        write: W,
    ) -> Result<Self, SourceError> {
        // TODO: Allow for compressed representation (not just raw)
        let encoder: Encoder<_> = match encoder_type {
            EncoderType::Compressed => {
                #[cfg(feature = "compression")]
                {
                    let compression = CompressedOutput::new(
                        CodecMetadata {
                            codec_version: LATEST_CODEC_VERSION,
                            header_size: 0,
                            time_mode: time_mode.unwrap_or_default(),
                            plane: self.state.plane,
                            tps: self.state.tps,
                            ref_interval: self.state.ref_time,
                            delta_t_max: self.state.delta_t_max,
                            event_size: 0,
                            source_camera: source_camera.unwrap_or_default(),
                        },
                        write,
                    );
                    Encoder::new_compressed(compression, encoder_options)
                }
                #[cfg(not(feature = "compression"))]
                {
                    return Err(SourceError::BadParams(
                        "Compressed representation is experimental and is not enabled by default!"
                            .to_string(),
                    ));
                }
            }
            EncoderType::Raw => {
                let compression = RawOutput::new(
                    CodecMetadata {
                        codec_version: LATEST_CODEC_VERSION,
                        header_size: 0,
                        time_mode: time_mode.unwrap_or_default(),
                        plane: self.state.plane,
                        tps: self.state.tps,
                        ref_interval: self.state.ref_time,
                        delta_t_max: self.state.delta_t_max,
                        event_size: 0,
                        source_camera: source_camera.unwrap_or_default(),
                    },
                    write,
                );
                Encoder::new_raw(compression, encoder_options)
            }
            EncoderType::Empty => {
                let compression = EmptyOutput::new(
                    CodecMetadata {
                        codec_version: LATEST_CODEC_VERSION,
                        header_size: 0,
                        time_mode: time_mode.unwrap_or_default(),
                        plane: self.state.plane,
                        tps: self.state.tps,
                        ref_interval: self.state.ref_time,
                        delta_t_max: self.state.delta_t_max,
                        event_size: 0,
                        source_camera: source_camera.unwrap_or_default(),
                    },
                    sink(),
                );
                Encoder::new_empty(compression)
            }
        };

        self.encoder = encoder;
        self.encoder_type = encoder_type;

        self.event_pixel_trees.par_map_inplace(|px| {
            px.time_mode(time_mode);
        });
        Ok(self)
    }

    /// Set the display mode for the instantaneous view.
    pub fn show_display(mut self, show_display: bool) -> Self {
        self.state.show_display = show_display;
        self
    }

    /// Close and flush the stream writer.
    /// # Errors
    /// Returns an error if the stream writer cannot be closed cleanly.
    pub fn end_write_stream(&mut self) -> Result<(), SourceError> {
        let mut tmp: Encoder<W> =
            Encoder::new_empty(EmptyOutput::new(CodecMetadata::default(), sink()));
        swap(&mut self.encoder, &mut tmp);
        tmp.close_writer()?;
        Ok(())
    }

    #[allow(clippy::needless_pass_by_value)]
    pub(crate) fn integrate_matrix(
        &mut self,
        matrix: Frame,
        time_spanned: f32,
        view_interval: u32,
    ) -> Result<Vec<Vec<Event>>, SourceError> {
        if self.state.in_interval_count == 0 {
            self.set_initial_d(&matrix);
        }

        self.state.in_interval_count += 1;

        self.state.show_live = self.state.in_interval_count % view_interval == 0;

        let px_per_chunk: usize = self.state.chunk_rows * self.state.plane.area_wc();

        // Important: if framing the events simultaneously, then the chunk division must be
        // exactly the same as it is for the framer
        let big_buffer: Vec<Vec<Event>> = self
            .event_pixel_trees
            .axis_chunks_iter_mut(Axis(0), self.state.chunk_rows)
            .into_par_iter()
            .zip(
                matrix
                    .axis_chunks_iter(Axis(0), self.state.chunk_rows)
                    .into_par_iter(),
            )
            .map(|(mut px_chunk, matrix_chunk)| {
                let mut buffer: Vec<Event> = Vec::with_capacity(px_per_chunk);
                let bump = Bump::new();
                let base_val = bump.alloc(0);
                let frame_val = bump.alloc(0);
                let frame_val_intensity32 = bump.alloc(0.0);

                for (px, input) in px_chunk.iter_mut().zip(matrix_chunk.iter()) {
                    *frame_val_intensity32 =
                        (f64::from(*input) * self.state.ref_time_divisor) as Intensity32;
                    *frame_val = *frame_val_intensity32 as u8;

                    integrate_for_px(
                        px,
                        base_val,
                        frame_val,
                        *frame_val_intensity32, // In this case, frame val is the same as intensity to integrate
                        time_spanned,
                        &mut buffer,
                        &self.state,
                    );
                }
                buffer
            })
            .collect();

        let db = match self.display_frame.as_slice_mut() {
            Some(v) => v,
            None => {
                return Err(SourceError::VisionError(
                    "Could not convert frame to slice".to_string(),
                ));
            }
        };

        // TODO: When there's full support for various bit-depth sources, modify this accordingly
        let practical_d_max =
            fast_math::log2_raw(255.0 * (self.state.delta_t_max / self.state.ref_time) as f32);

        // Zip::indexed(&self.running_intensities)
        //     .and(db)
        //     .par_for_each(|idx, val| {});

        db.iter_mut()
            .zip(self.state.running_intensities.iter_mut())
            .enumerate()
            .for_each(|(idx, (val, running))| {
                let y = idx / self.state.plane.area_wc();
                let x = (idx % self.state.plane.area_wc()) / self.state.plane.c_usize();
                let c = idx % self.state.plane.c_usize();

                let sae_time = self.event_pixel_trees[[y, x, c]].last_fired_t;

                // Set the instantaneous frame value to the best queue'd event for the pixel
                *val = match self.event_pixel_trees[[y, x, c]].arena[0].best_event {
                    Some(event) => u8::get_frame_value(
                        &event.into(),
                        SourceType::U8,
                        self.state.ref_time as DeltaT,
                        practical_d_max,
                        self.state.delta_t_max,
                        self.instantaneous_view_mode,
                        if self.instantaneous_view_mode == SAE {
                            Some(SaeTime {
                                running_t: self.event_pixel_trees[[y, x, c]].running_t as DeltaT,
                                last_fired_t: self.event_pixel_trees[[y, x, c]].last_fired_t
                                    as DeltaT,
                            })
                        } else {
                            None
                        },
                    ),
                    None => *val,
                };

                // Only track the running state of the first channel
                if self.state.feature_detection && c == 0 {
                    *running = *val;
                }
            });

        for events in &big_buffer {
            for (e1, e2) in events.iter().circular_tuple_windows() {
                self.encoder.ingest_event(*e1)?;
            }
        }

        self.handle_features(&big_buffer)?;

        if self.state.show_live {
            // show_display("instance", &self.instantaneous_frame, 1, self)?;
        }

        Ok(big_buffer)
    }

    fn set_initial_d(&mut self, frame: &Frame) {
        self.event_pixel_trees
            .axis_chunks_iter_mut(Axis(0), self.state.chunk_rows)
            .into_par_iter()
            .zip(
                frame
                    .axis_chunks_iter(Axis(0), self.state.chunk_rows)
                    .into_par_iter(),
            )
            .for_each(|(mut px, frame_chunk)| {
                for (px, frame_val) in px.iter_mut().zip(frame_chunk.iter()) {
                    let d_start = f32::from(*frame_val).log2().floor() as D;
                    px.arena[0].set_d(d_start);
                    px.base_val = *frame_val;
                }
            });
    }

    /// Get `ref_time`
    pub fn get_ref_time(&self) -> u32 {
        self.state.ref_time
    }

    /// Get `delta_t_max`
    pub fn get_delta_t_max(&self) -> u32 {
        self.state.delta_t_max
    }

    /// Get `tps`
    pub fn get_tps(&self) -> u32 {
        self.state.tps
    }

    /// Set a new value for `delta_t_max`
    pub fn update_delta_t_max(&mut self, dtm: u32) {
        // Validate new value
        self.state.delta_t_max = self.state.ref_time.max(dtm);
    }

    /// Set a new bool for `feature_detection`
    pub fn update_detect_features(
        &mut self,
        detect_features: bool,
        show_features: ShowFeatureMode,
    ) {
        // Validate new value
        self.state.feature_detection = detect_features;
        self.state.show_features = show_features;
    }

    /// Set a new value for `c_thresh_pos`
    #[deprecated(
        since = "0.3.4",
        note = "please use `update_crf` or `update_quality_manual` instead"
    )]
    pub fn update_adder_thresh_pos(&mut self, c: u8) {
        for px in self.event_pixel_trees.iter_mut() {
            px.c_thresh = c;
        }
        self.state.c_thresh_baseline = c;
    }

    /// Set a new value for `c_thresh_neg`
    #[deprecated(
        since = "0.3.4",
        note = "please use `update_crf` or `update_quality_manual` instead"
    )]
    pub fn update_adder_thresh_neg(&mut self, _c: u8) {
        unimplemented!();
        // for px in self.event_pixel_trees.iter_mut() {
        //     px.c_thresh = c;
        // }
        // self.state.c_thresh_neg = c;
    }

    pub(crate) fn handle_features(
        &mut self,
        big_buffer: &Vec<Vec<Event>>,
    ) -> Result<(), SourceError> {
        if !self.state.feature_detection {
            return Ok(()); // Early return
        }
        let mut new_features: Vec<Vec<Coord>> =
            vec![Vec::with_capacity(self.state.features[0].len()); self.state.features.len()];

        let start = Instant::now();

        big_buffer
            .par_iter()
            .zip(self.state.features.par_iter_mut())
            .zip(new_features.par_iter_mut())
            .for_each(|((events, feature_set), new_features)| {
                for (e1, e2) in events.iter().circular_tuple_windows() {
                    if e1.coord.c == None || e1.coord.c == Some(0) {
                        if e1.coord != e2.coord
                            && (!cfg!(feature = "feature-logging-nonmaxsuppression")
                                || e2.delta_t != e1.delta_t)
                        {
                            if is_feature(
                                e1.coord,
                                self.state.plane,
                                &self.state.running_intensities,
                            )
                            .unwrap()
                            {
                                if feature_set.insert(e1.coord) {
                                    new_features.push(e1.coord);
                                };
                            } else {
                                feature_set.remove(&e1.coord);
                            }
                        }
                    }
                }
            });

        #[cfg(feature = "feature-logging")]
        {
            let total_duration_nanos = start.elapsed().as_nanos();

            if let Some(handle) = &mut self.state.feature_log_handle {
                for feature_set in &self.state.features {
                    for (coord) in feature_set {
                        let bytes = serde_pickle::to_vec(
                            &LogFeature::from_coord(
                                *coord,
                                LogFeatureSource::ADDER,
                                cfg!(feature = "feature-logging-nonmaxsuppression"),
                            ),
                            Default::default(),
                        )
                        .unwrap();
                        handle.write_all(&bytes).unwrap();
                    }
                }

                let out = format!("\nADDER FAST: {}", total_duration_nanos);
                handle
                    .write_all(&serde_pickle::to_vec(&out, Default::default()).unwrap())
                    .unwrap();
            }

            // Convert the running intensities to a Mat
            let cv_type = match self.state.running_intensities.shape()[2] {
                1 => opencv::core::CV_8UC1,
                _ => opencv::core::CV_8UC3,
            };

            let mut cv_mat = unsafe {
                let RawParts {
                    ptr,
                    length,
                    capacity,
                } = RawParts::from_vec(self.display_frame.clone().into_raw_vec()); // pixels will be move into_raw_parts，and return a manually drop pointer.
                let mut cv_mat = opencv::core::Mat::new_rows_cols_with_data(
                    self.state.plane.h() as i32,
                    self.state.plane.w() as i32,
                    cv_type,
                    ptr as *mut c_void,
                    opencv::core::Mat_AUTO_STEP,
                )
                .unwrap();
                cv_mat.addref().unwrap(); // ???

                cv_mat
            };

            let tmp = cv_mat.clone();
            if cv_type == opencv::core::CV_8UC3 {
                opencv::imgproc::cvt_color(&tmp, &mut cv_mat, opencv::imgproc::COLOR_BGR2GRAY, 0)?;
            }

            let start = Instant::now();
            let mut keypoints = Vector::<KeyPoint>::new();

            opencv::features2d::fast(
                &cv_mat,
                &mut keypoints,
                crate::utils::cv::INTENSITY_THRESHOLD.into(),
                cfg!(feature = "feature-logging-nonmaxsuppression"),
            )?;

            let duration = start.elapsed();
            if let Some(handle) = &mut self.state.feature_log_handle {
                for keypoint in &keypoints {
                    let bytes = serde_pickle::to_vec(
                        &LogFeature::from_keypoint(
                            &keypoint,
                            LogFeatureSource::OpenCV,
                            cfg!(feature = "feature-logging-nonmaxsuppression"),
                        ),
                        Default::default(),
                    )
                    .unwrap();
                    handle.write_all(&bytes).unwrap();
                }

                let out = format!("\nOpenCV FAST: {}", duration.as_nanos());
                handle
                    .write_all(&serde_pickle::to_vec(&out, Default::default()).unwrap())
                    .unwrap();
            }
            let mut keypoint_mat = Mat::default();
            opencv::features2d::draw_keypoints(
                &cv_mat,
                &keypoints,
                &mut keypoint_mat,
                Scalar::new(0.0, 0.0, 255.0, 0.0),
                opencv::features2d::DrawMatchesFlags::DEFAULT,
            )?;
            show_display_force("keypoints", &keypoint_mat, 1)?;
        }

        if self.state.show_features == ShowFeatureMode::Hold {
            // Display the feature on the viz frame
            for feature_set in &self.state.features {
                for (coord) in feature_set {
                    draw_feature_coord(
                        coord.x,
                        coord.y,
                        &mut self.display_frame,
                        self.state.plane.c() != 1,
                    );
                }
            }
        }

        for feature_set in new_features {
            for (coord) in feature_set {
                if self.state.show_features == ShowFeatureMode::Instant {
                    draw_feature_coord(
                        coord.x,
                        coord.y,
                        &mut self.display_frame,
                        self.state.plane.c() != 1,
                    );
                }
                let radius = self.state.feature_c_radius as i32;
                for r in (coord.y() as i32 - radius).max(0)
                    ..(coord.y() as i32 + radius).min(self.state.plane.h() as i32)
                {
                    for c in (coord.x() as i32 - radius).max(0)
                        ..(coord.x() as i32 + radius).min(self.state.plane.w() as i32)
                    {
                        self.event_pixel_trees[[r as usize, c as usize, coord.c_usize()]]
                            .c_thresh = self.state.c_thresh_baseline;
                    }
                }
            }
        }

        Ok(())
    }

    /// Set whether or not to detect features, and whether or not to display the features
    pub fn detect_features(
        mut self,
        detect_features: bool,
        show_features: ShowFeatureMode,
    ) -> Self {
        self.state.feature_detection = detect_features;
        self.state.show_features = show_features;
        self
    }

    /// Update the CRF value and set the baseline c for all pixels
    pub(crate) fn update_crf(&mut self, crf: u8, update_time_params: bool) {
        self.state.update_crf(crf, update_time_params);

        for px in self.event_pixel_trees.iter_mut() {
            px.c_thresh = self.state.c_thresh_baseline;
            px.c_increase_counter = 0;
        }
    }

    pub fn get_encoder_options(&self) -> EncoderOptions {
        self.encoder.get_options()
    }
    pub fn get_time_mode(&self) -> TimeMode {
        self.encoder.meta().time_mode
    }

    /// Manually set the parameters dictating quality
    pub fn update_quality_manual(
        &mut self,
        c_thresh_baseline: u8,
        c_thresh_max: u8,
        delta_t_max_multiplier: u32,
        c_increase_velocity: u8,
        feature_c_radius_denom: f32,
    ) {
        self.state.update_quality_manual(
            c_thresh_baseline,
            c_thresh_max,
            delta_t_max_multiplier,
            c_increase_velocity,
            feature_c_radius_denom,
        );

        for px in self.event_pixel_trees.iter_mut() {
            px.c_thresh = c_thresh_baseline;
            px.c_increase_counter = 0;
        }
    }

    pub fn get_event_size(&self) -> u8 {
        self.encoder.meta().event_size
    }
}

/// Integrate an intensity value for a pixel, over a given time span
///
/// # Arguments
///
/// * `px`: the pixel to integrate
/// * `base_val`: holder for the base intensity value of the pixel
/// * `frame_val`: the intensity value, normalized to a fixed-length period defined by `ref_time`.
/// Used for determining if the pixel must pop its events.
/// * `intensity`: the intensity to integrate
/// * `time_spanned`: the time spanned by the intensity value
/// * `buffer`: the buffer to push events to
/// * `state`: the state of the video source
///
/// returns: ()
#[inline(always)]
pub fn integrate_for_px(
    px: &mut PixelArena,
    base_val: &mut u8,
    frame_val: &u8,
    intensity: Intensity32,
    time_spanned: f32,
    buffer: &mut Vec<Event>,
    state: &VideoState,
) -> bool {
    let mut grew_buffer = false;
    if px.need_to_pop_top {
        buffer.push(px.pop_top_event(intensity, state.pixel_tree_mode, state.ref_time));
        grew_buffer = true;
    }

    *base_val = px.base_val;

    if *frame_val < base_val.saturating_sub(px.c_thresh)
        || *frame_val > base_val.saturating_add(px.c_thresh)
    {
        px.pop_best_events(buffer, state.pixel_tree_mode, state.ref_time);
        grew_buffer = true;
        px.base_val = *frame_val;

        // If continuous mode and the D value needs to be different now
        if let Continuous = state.pixel_tree_mode {
            match px.set_d_for_continuous(intensity, state.ref_time) {
                None => {}
                Some(event) => buffer.push(event),
            };
        }
    }

    px.integrate(
        intensity,
        time_spanned.into(),
        state.pixel_tree_mode,
        state.delta_t_max,
        state.ref_time,
        state.c_thresh_max,
        state.c_increase_velocity,
    );

    if px.need_to_pop_top {
        buffer.push(px.pop_top_event(intensity, state.pixel_tree_mode, state.ref_time));
        grew_buffer = true;
    }
    grew_buffer
}

#[cfg(feature = "open-cv")]
/// If `video.show_display`, shows the given [`Mat`] in an `OpenCV` window
/// with the given name.
///
/// # Errors
/// Returns an [`opencv::Error`] if the window cannot be shown, or the [`Mat`] cannot be scaled as
/// needed.
pub fn show_display<W: Write>(
    window_name: &str,
    mat: &Mat,
    wait: i32,
    video: &Video<W>,
) -> opencv::Result<()> {
    if video.state.show_display {
        show_display_force(window_name, mat, wait)?;
    }
    Ok(())
}

#[cfg(feature = "open-cv")]
/// Shows the given [`Mat`] in an `OpenCV` window with the given name.
/// This function is the same as [`show_display`], except that it does not check
/// [`Video::show_display`].
/// This function is useful for debugging.
/// # Errors
/// Returns an [`opencv::Error`] if the window cannot be shown, or the [`Mat`] cannot be scaled as
/// needed.
pub fn show_display_force(window_name: &str, mat: &Mat, wait: i32) -> opencv::Result<()> {
    let mut tmp = Mat::default();

    if mat.rows() == 940 {
        highgui::imshow(window_name, mat)?;
    } else {
        let factor = mat.rows() as f32 / 940.0;
        resize(
            mat,
            &mut tmp,
            Size {
                width: (mat.cols() as f32 / factor) as i32,
                height: 940,
            },
            0.0,
            0.0,
            0,
        )?;
        highgui::imshow(window_name, &tmp)?;
    }

    highgui::wait_key(wait)?;
    Ok(())
}

/// A trait for objects that can be used as a source of data for the ADΔER transcode model.
pub trait Source<W: Write> {
    /// Intake one input interval worth of data from the source stream into the ADΔER model as
    /// intensities.
    fn consume(
        &mut self,
        view_interval: u32,
        thread_pool: &ThreadPool,
    ) -> Result<Vec<Vec<Event>>, SourceError>;

    /// Set the Constant Rate Factor (CRF) quality setting for the encoder. 0 is lossless, 9 is worst quality.
    fn crf(&mut self, crf: u8);

    /// Get a mutable reference to the [`Video`] object associated with this [`Source`].
    fn get_video_mut(&mut self) -> &mut Video<W>;

    /// Get an immutable reference to the [`Video`] object associated with this [`Source`].
    fn get_video_ref(&self) -> &Video<W>;

    /// Get the [`Video`] object associated with this [`Source`], consuming the [`Source`] in the
    /// process.
    fn get_video(self) -> Video<W>;

    fn get_input(&self) -> Option<&Frame>;

    /// Get the last-calculated bitrate of the input (in bits per second)
    fn get_running_input_bitrate(&self) -> f64;
}
