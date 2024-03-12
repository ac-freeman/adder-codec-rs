#[cfg(feature = "open-cv")]
use opencv::core::{Mat, Size};
#[cfg(feature = "opencv")]
use opencv::prelude::*;
use std::cmp::min;
use std::collections::HashSet;
#[cfg(feature = "feature-logging")]
use std::ffi::c_void;
use std::io::{sink, Write};
use std::mem::swap;

use adder_codec_core::codec::empty::stream::EmptyOutput;
use adder_codec_core::codec::encoder::Encoder;
use adder_codec_core::codec::raw::stream::RawOutput;
use adder_codec_core::codec::{
    CodecError, CodecMetadata, EncoderOptions, EncoderType, LATEST_CODEC_VERSION,
};
use adder_codec_core::{
    Coord, DeltaT, Event, Mode, PixelAddress, PixelMultiMode, PlaneError, PlaneSize, SourceCamera,
    SourceType, TimeMode, D_EMPTY,
};
use bumpalo::Bump;

use std::sync::mpsc::{channel, Sender};
use std::time::Instant;

use crate::framer::scale_intensity::{FrameValue, SaeTime};
use crate::transcoder::event_pixel_tree::{Intensity32, PixelArena};
use adder_codec_core::D;
#[cfg(feature = "opencv")]
use davis_edi_rs::util::reconstructor::ReconstructionError;
#[cfg(feature = "opencv")]
use opencv::{highgui, imgproc::resize};

#[cfg(feature = "compression")]
use adder_codec_core::codec::compressed::stream::CompressedOutput;
use adder_codec_core::Mode::Continuous;
use itertools::Itertools;
use ndarray::{Array, Array3, Axis, ShapeError};
use rayon::iter::IndexedParallelIterator;
use rayon::iter::IntoParallelIterator;
use rayon::iter::ParallelIterator;
use rayon::ThreadPool;

use crate::transcoder::source::video::FramedViewMode::SAE;
use crate::utils::cv::is_feature;

use crate::utils::viz::{draw_feature_coord, draw_rect, ShowFeatureMode};
use adder_codec_core::codec::rate_controller::{Crf, CrfParameters};
use kiddo::{KdTree, SquaredEuclidean};
use thiserror::Error;
use tokio::task::JoinError;
use video_rs_adder_dep::Frame;

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
    VideoError(video_rs_adder_dep::Error),

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

    /// I/O error
    #[error("I/O error")]
    IoError(#[from] std::io::Error),
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

impl From<video_rs_adder_dep::Error> for SourceError {
    fn from(value: video_rs_adder_dep::Error) -> Self {
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

#[derive(Debug)]
pub struct VideoStateParams {
    pub(crate) pixel_tree_mode: Mode,

    pub pixel_multi_mode: PixelMultiMode,

    /// The maximum time difference between events of the same pixel, in ticks
    pub delta_t_max: u32,

    /// The reference time in ticks
    pub ref_time: u32,
}

impl Default for VideoStateParams {
    fn default() -> Self {
        Self {
            pixel_tree_mode: Continuous,
            pixel_multi_mode: Default::default(),
            delta_t_max: 7650,
            ref_time: 255,
        }
    }
}

/// Running state of the video transcode
#[derive(Debug)]
pub struct VideoState {
    pub params: VideoStateParams,

    /// The size of the imaging plane
    pub plane: PlaneSize,

    /// The number of rows of pixels to process at a time (per thread)
    pub chunk_rows: usize,

    /// The number of input intervals (of fixed time) processed so far
    pub in_interval_count: u32,
    // pub(crate) c_thresh_pos: u8,
    // pub(crate) c_thresh_neg: u8,
    pub(crate) ref_time_divisor: f32,
    pub tps: DeltaT,

    pub(crate) show_display: bool,
    pub(crate) show_live: bool,

    /// Whether or not to detect features
    pub feature_detection: bool,

    /// The current instantaneous frame, for determining features
    pub running_intensities: Array3<u8>,

    /// Whether or not to draw the features on the display mat, and the mode to do it in
    show_features: ShowFeatureMode,

    features: Vec<HashSet<Coord>>,

    pub feature_log_handle: Option<std::fs::File>,
}

impl Default for VideoState {
    fn default() -> Self {
        VideoState {
            plane: PlaneSize::default(),
            params: VideoStateParams::default(),
            chunk_rows: 1,
            in_interval_count: 1,
            ref_time_divisor: 1.0,
            tps: 7650,
            show_display: false,
            show_live: false,
            feature_detection: false,
            running_intensities: Default::default(),
            show_features: ShowFeatureMode::Off,
            features: Default::default(),
            feature_log_handle: None,
        }
    }
}

// impl VideoState {
//     fn update_crf(&mut self, crf: u8) {
//         self.crf_quality = crf;
//         self.c_thresh_baseline = CRF[crf as usize][0] as u8;
//         self.c_thresh_max = CRF[crf as usize][1] as u8;
//
//         self.c_increase_velocity = CRF[crf as usize][2] as u8;
//         self.feature_c_radius = (CRF[crf as usize][3] * self.plane.min_resolution() as f32) as u16;
//     }
//
//     fn update_quality_manual(
//         &mut self,
//         c_thresh_baseline: u8,
//         c_thresh_max: u8,
//         delta_t_max_multiplier: u32,
//         c_increase_velocity: u8,
//         feature_c_radius: f32,
//     ) {
//         self.c_thresh_baseline = c_thresh_baseline;
//         self.c_thresh_max = c_thresh_max;
//         self.delta_t_max = delta_t_max_multiplier * self.ref_time;
//         self.c_increase_velocity = c_increase_velocity;
//         self.feature_c_radius = feature_c_radius as u16; // The absolute pixel count radius
//     }
// }

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
        pixel_multi_mode: PixelMultiMode,
        adu_interval: Option<usize>,
        encoder_type: EncoderType,
        encoder_options: EncoderOptions,
        write: W,
    ) -> Result<Box<Self>, SourceError>;

    /// Set whether or not the show the live display
    fn show_display(self, show_display: bool) -> Self;

    /// Set whether or not to detect features, and whether or not to display the features
    fn detect_features(self, detect_features: bool, show_features: ShowFeatureMode) -> Self;

    #[cfg(feature = "feature-logging")]
    fn log_path(self, name: String) -> Self;
}

// impl VideoBuilder for Video {}

/// Attributes common to ADΔER transcode process
pub struct Video<W: Write> {
    /// The current state of the video transcode
    pub state: VideoState,
    pub(crate) event_pixel_trees: Array3<PixelArena>,

    /// The current instantaneous display frame with the features drawn on it
    pub display_frame_features: Frame,

    /// The current view mode of the instantaneous frame
    pub instantaneous_view_mode: FramedViewMode,

    /// Channel for sending events to the encoder
    pub event_sender: Sender<Vec<Event>>,
    pub encoder: Encoder<W>,

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
            params: VideoStateParams {
                pixel_tree_mode,
                ..Default::default()
            },
            running_intensities: Array::zeros((plane.h_usize(), plane.w_usize(), plane.c_usize())),
            ..Default::default()
        };

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
        let instantaneous_frame =
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
            ref_interval: state.params.ref_time,
            delta_t_max: state.params.delta_t_max,
            event_size: 0,
            source_camera: SourceCamera::default(), // TODO: Allow for setting this
            adu_interval: Default::default(),
        };

        match writer {
            None => {
                let encoder: Encoder<W> = Encoder::new_empty(
                    EmptyOutput::new(meta, sink()),
                    EncoderOptions::default(state.plane),
                );
                Ok(Video {
                    state,
                    event_pixel_trees,
                    display_frame_features: instantaneous_frame,
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
                    EncoderOptions::default(state.plane),
                );
                Ok(Video {
                    state,
                    event_pixel_trees,
                    display_frame_features: instantaneous_frame,
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
        dbg!("t");
        self.encoder
            .options
            .crf
            .override_c_thresh_baseline(c_thresh_pos);
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
                ref_time, self.state.params.ref_time
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
                delta_t_max, self.state.params.delta_t_max
            );
            return Ok(self);
        }
        if delta_t_max < ref_time {
            eprintln!(
                "Delta t max {} is smaller than reference time {}. Keeping current value of {}.",
                delta_t_max, ref_time, self.state.params.delta_t_max
            );
            return Ok(self);
        }
        self.state.params.delta_t_max = delta_t_max;
        self.state.params.ref_time = ref_time;
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
        pixel_multi_mode: Option<PixelMultiMode>,
        adu_interval: Option<usize>,
        encoder_type: EncoderType,
        encoder_options: EncoderOptions,
        write: W,
    ) -> Result<Self, SourceError> {
        let encoder: Encoder<_> = match encoder_type {
            EncoderType::Compressed => {
                #[cfg(feature = "compression")]
                {
                    self.state.params.pixel_multi_mode =
                        pixel_multi_mode.unwrap_or(PixelMultiMode::Collapse);
                    let compression = CompressedOutput::new(
                        CodecMetadata {
                            codec_version: LATEST_CODEC_VERSION,
                            header_size: 0,
                            time_mode: time_mode.unwrap_or_default(),
                            plane: self.state.plane,
                            tps: self.state.tps,
                            ref_interval: self.state.params.ref_time,
                            delta_t_max: self.state.params.delta_t_max,
                            event_size: 0,
                            source_camera: source_camera.unwrap_or_default(),
                            adu_interval: adu_interval.unwrap_or_default(),
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
                self.state.params.pixel_multi_mode =
                    pixel_multi_mode.unwrap_or(PixelMultiMode::Collapse);
                let compression = RawOutput::new(
                    CodecMetadata {
                        codec_version: LATEST_CODEC_VERSION,
                        header_size: 0,
                        time_mode: time_mode.unwrap_or_default(),
                        plane: self.state.plane,
                        tps: self.state.tps,
                        ref_interval: self.state.params.ref_time,
                        delta_t_max: self.state.params.delta_t_max,
                        event_size: 0,
                        source_camera: source_camera.unwrap_or_default(),
                        adu_interval: Default::default(),
                    },
                    write,
                );
                Encoder::new_raw(compression, encoder_options)
            }
            EncoderType::Empty => {
                self.state.params.pixel_multi_mode =
                    pixel_multi_mode.unwrap_or(PixelMultiMode::Collapse);
                let compression = EmptyOutput::new(
                    CodecMetadata {
                        codec_version: LATEST_CODEC_VERSION,
                        header_size: 0,
                        time_mode: time_mode.unwrap_or_default(),
                        plane: self.state.plane,
                        tps: self.state.tps,
                        ref_interval: self.state.params.ref_time,
                        delta_t_max: self.state.params.delta_t_max,
                        event_size: 0,
                        source_camera: source_camera.unwrap_or_default(),
                        adu_interval: Default::default(),
                    },
                    sink(),
                );
                Encoder::new_empty(compression, encoder_options)
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
    pub fn end_write_stream(&mut self) -> Result<Option<W>, SourceError> {
        let mut tmp: Encoder<W> = Encoder::new_empty(
            EmptyOutput::new(CodecMetadata::default(), sink()),
            self.encoder.options,
        );
        swap(&mut self.encoder, &mut tmp);
        Ok(tmp.close_writer()?)
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

        let parameters = *self.encoder.options.crf.get_parameters();

        self.state.in_interval_count += 1;

        self.state.show_live = self.state.in_interval_count % view_interval == 0;

        // let matrix_f32 = convert_u8_to_f32_simd(&matrix.into_raw_vec());
        let matrix = matrix.mapv(f32::from);

        // TODO: When there's full support for various bit-depth sources, modify this accordingly
        let practical_d_max = fast_math::log2_raw(
            255.0 * (self.state.params.delta_t_max / self.state.params.ref_time) as f32,
        );

        let tpf = self.state.params.ref_time as f64;

        let params = &self.state.params;
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
            .zip(
                self.state
                    .running_intensities
                    .axis_chunks_iter_mut(Axis(0), self.state.chunk_rows)
                    .into_par_iter(),
            )
            .map(|((mut px_chunk, matrix_chunk), mut running_chunk)| {
                let mut buffer: Vec<Event> = Vec::with_capacity(10);
                let bump = Bump::new();
                let base_val = bump.alloc(0);

                for ((px, input), running) in px_chunk
                    .iter_mut()
                    .zip(matrix_chunk.iter())
                    .zip(running_chunk.iter_mut())
                {
                    integrate_for_px(
                        px,
                        base_val,
                        *input as u8,
                        *input, // In this case, frame val is the same as intensity to integrate
                        time_spanned,
                        &mut buffer,
                        params,
                        &parameters,
                    );

                    if let Some(event) = px.arena[0].best_event {
                        *running = u8::get_frame_value(
                            &event.into(),
                            SourceType::U8,
                            tpf,
                            practical_d_max,
                            self.state.params.delta_t_max,
                            self.instantaneous_view_mode,
                            if self.instantaneous_view_mode == SAE {
                                Some(SaeTime {
                                    running_t: px.running_t as DeltaT,
                                    last_fired_t: px.last_fired_t as DeltaT,
                                })
                            } else {
                                None
                            },
                        );
                    };
                }
                buffer
            })
            .collect();

        for events in &big_buffer {
            for e1 in events.iter() {
                self.encoder.ingest_event(*e1)?;
            }
        }

        self.display_frame_features = self.state.running_intensities.clone();

        self.handle_features(&big_buffer)?;

        #[cfg(feature = "feature-logging")]
        {
            if let Some(handle) = &mut self.state.feature_log_handle {
                // Calculate current bitrate
                let mut events_per_sec = 0.0;
                for events_vec in &big_buffer {
                    events_per_sec += events_vec.len() as f64;
                }

                events_per_sec *= self.state.tps as f64 / self.state.params.ref_time as f64;

                let bitrate =
                    events_per_sec * if self.state.plane.c() == 1 { 9.0 } else { 11.0 } * 8.0;

                handle
                    .write_all(
                        &serde_pickle::to_vec(&format!("\nbps: {}", bitrate), Default::default())
                            .unwrap(),
                    )
                    .unwrap();

                handle
                    .write_all(
                        &serde_pickle::to_vec(&"\n".to_string(), Default::default()).unwrap(),
                    )
                    .unwrap();
            }
        }

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
        self.state.params.ref_time
    }

    /// Get `delta_t_max`
    pub fn get_delta_t_max(&self) -> u32 {
        self.state.params.delta_t_max
    }

    /// Get `tps`
    pub fn get_tps(&self) -> u32 {
        self.state.tps
    }

    /// Set a new value for `delta_t_max`
    pub fn update_delta_t_max(&mut self, dtm: u32) {
        // Validate new value
        self.state.params.delta_t_max = self.state.params.ref_time.max(dtm);
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
        dbg!("t1");
        self.encoder.options.crf.override_c_thresh_baseline(c)
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

    pub(crate) fn handle_features(&mut self, big_buffer: &[Vec<Event>]) -> Result<(), SourceError> {
        // if !cfg!(feature = "feature-logging") && !self.state.feature_detection {
        if !self.state.feature_detection {
            return Ok(()); // Early return
        }
        let mut new_features: Vec<Vec<Coord>> =
            vec![Vec::with_capacity(self.state.features[0].len()); self.state.features.len()];

        let _start = Instant::now();

        big_buffer
            // .par_iter()
            // .zip(self.state.features.par_iter_mut())
            // .zip(new_features.par_iter_mut())
            .iter()
            .zip(self.state.features.iter_mut())
            .zip(new_features.iter_mut())
            .for_each(|((events, feature_set), new_features)| {
                for (e1, e2) in events.iter().circular_tuple_windows() {
                    if (e1.coord.c.is_none() || e1.coord.c == Some(0))
                        && e1.coord != e2.coord
                        && (!cfg!(feature = "feature-logging-nonmaxsuppression") || e2.t != e1.t)
                        && e1.d != D_EMPTY
                    {
                        if is_feature(e1.coord, self.state.plane, &self.state.running_intensities)
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
            });

        let mut new_features = new_features.iter()
            .flat_map(|feature_set| feature_set.iter().map(|coord| [coord.x, coord.y])).collect::<Vec<[u16;2]>>();
        let mut new_features: HashSet<[u16;2]> = new_features.drain(..).collect();

        #[cfg(feature = "feature-logging")]
        {
            let total_duration_nanos = _start.elapsed().as_nanos();

            if let Some(handle) = &mut self.state.feature_log_handle {
                for feature_set in &self.state.features {
                    // for (coord) in feature_set {
                    //     let bytes = serde_pickle::to_vec(
                    //         &LogFeature::from_coord(
                    //             *coord,
                    //             LogFeatureSource::ADDER,
                    //             cfg!(feature = "feature-logging-nonmaxsuppression"),
                    //         ),
                    //         Default::default(),
                    //     )
                    //     .unwrap();
                    //     handle.write_all(&bytes).unwrap();
                    // }
                    handle
                        .write_all(
                            &serde_pickle::to_vec(&feature_set.len(), Default::default()).unwrap(),
                        )
                        .unwrap();
                }

                let out = format!("\nADDER FAST: {}\n", total_duration_nanos);
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
                let raw_parts::RawParts {
                    ptr,
                    length: _,
                    capacity: _,
                } = raw_parts::RawParts::from_vec(
                    self.display_frame_features.clone().into_raw_vec(),
                ); // pixels will be move into_raw_parts，and return a manually drop pointer.
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
            let mut keypoints = opencv::core::Vector::<opencv::core::KeyPoint>::new();

            opencv::features2d::fast(
                &cv_mat,
                &mut keypoints,
                crate::utils::cv::INTENSITY_THRESHOLD.into(),
                cfg!(feature = "feature-logging-nonmaxsuppression"),
            )?;

            let duration = start.elapsed();
            if let Some(handle) = &mut self.state.feature_log_handle {
                // for keypoint in &keypoints {
                //     let bytes = serde_pickle::to_vec(
                //         &LogFeature::from_keypoint(
                //             &keypoint,
                //             LogFeatureSource::OpenCV,
                //             cfg!(feature = "feature-logging-nonmaxsuppression"),
                //         ),
                //         Default::default(),
                //     )
                //     .unwrap();
                //     handle.write_all(&bytes).unwrap();
                // }
                handle
                    .write_all(&serde_pickle::to_vec(&keypoints.len(), Default::default()).unwrap())
                    .unwrap();

                let out = format!("\nOpenCV FAST: {}\n", duration.as_nanos());
                handle
                    .write_all(&serde_pickle::to_vec(&out, Default::default()).unwrap())
                    .unwrap();

                // Combine self.state.features into one hashset:
                let mut combined_features = HashSet::new();
                for feature_set in &self.state.features {
                    for coord in feature_set {
                        combined_features.insert(*coord);
                    }
                }
                let (precision, recall, accuracy) =
                    crate::utils::cv::feature_precision_recall_accuracy(
                        &keypoints,
                        &combined_features,
                        self.state.plane,
                    );
                let out = "\nFeature results: \n".to_string();
                handle
                    .write_all(&serde_pickle::to_vec(&out, Default::default()).unwrap())
                    .unwrap();
                handle
                    .write_all(&serde_pickle::to_vec(&precision, Default::default()).unwrap())
                    .unwrap();
                handle
                    .write_all(&serde_pickle::to_vec(&recall, Default::default()).unwrap())
                    .unwrap();
                handle
                    .write_all(&serde_pickle::to_vec(&accuracy, Default::default()).unwrap())
                    .unwrap();
            }

            let mut keypoint_mat = Mat::default();
            opencv::features2d::draw_keypoints(
                &cv_mat,
                &keypoints,
                &mut keypoint_mat,
                opencv::core::Scalar::new(0.0, 0.0, 255.0, 0.0),
                opencv::features2d::DrawMatchesFlags::DEFAULT,
            )?;

            // show_display_force("keypoints", &keypoint_mat, 1)?;
        }

        if self.state.show_features == ShowFeatureMode::Hold {
            // Display the feature on the viz frame
            for feature_set in &self.state.features {
                for coord in feature_set {
                    draw_feature_coord(
                        coord.x,
                        coord.y,
                        &mut self.display_frame_features,
                        self.state.plane.c() != 1,
                        None,
                    );
                }
            }
        }

        let parameters = self.encoder.options.crf.get_parameters();

        if parameters.feature_c_radius > 0 {
            for coord in &new_features {

                    if self.state.show_features == ShowFeatureMode::Instant {
                        draw_feature_coord(
                            coord[0],
                            coord[1],
                            &mut self.display_frame_features,
                            self.state.plane.c() != 1,
                            None,
                        );
                    }
                    let radius = parameters.feature_c_radius as i32;
                    for row in (coord[1] as i32 - radius).max(0)
                        ..=(coord[1] as i32 + radius).min(self.state.plane.h() as i32 - 1)
                    {
                        for col in (coord[0] as i32 - radius).max(0)
                            ..=(coord[0] as i32 + radius).min(self.state.plane.w() as i32 - 1)
                        {
                            for c in 0..self.state.plane.c() {
                                self.event_pixel_trees[[row as usize, col as usize, c as usize]]
                                    .c_thresh = min(parameters.c_thresh_baseline, 2);
                            }
                        }
                    }

            }
        }


        self.cluster(&new_features);

        Ok(())
    }

    fn cluster(&mut self, set: &HashSet<[u16; 2]>) {
        let points: Vec<[f32; 2]> = set
            .into_iter()
            .map(|coord| [coord[0] as f32, coord[1] as f32])
            .collect();
        let mut tree: KdTree<f32, 2> = (&points).into();

        if points.len() < 3 {
            return;
        }

        // DBSCAN algorithm to cluster the features

        let eps = 100.0;
        let min_pts = 3;

        let mut visited = vec![false; points.len()];
        let mut clusters = Vec::new();

        for (i, point) in points.iter().enumerate() {
            if visited[i] {
                continue;
            }
            visited[i] = true;

            let mut neighbors = tree.within_unsorted::<SquaredEuclidean>(point, eps);

            if neighbors.len() < min_pts {
                continue;
            }

            let mut cluster = HashSet::new();
            cluster.insert(i as u64);

            let mut index = 0;

            while index < neighbors.len() {
                let current_point = neighbors[index];
                if !visited[current_point.item as usize] {
                    visited[current_point.item as usize] = true;

                    let current_neighbors = tree.within_unsorted::<SquaredEuclidean>(
                        &points[current_point.item as usize],
                        eps,
                    );

                    if current_neighbors.len() >= min_pts {
                        neighbors.extend(
                            current_neighbors
                                .into_iter()
                                .filter(|&i| !cluster.contains(&i.item)),
                        );
                    }
                }

                if !cluster.contains(&current_point.item) {
                    cluster.insert(current_point.item);
                }

                index += 1;
            }

            clusters.push(cluster);
        }

        let mut bboxes = Vec::new();
        for cluster in clusters {
            let random_color = [
                rand::random::<u8>(),
                rand::random::<u8>(),
                rand::random::<u8>(),
            ];

            let mut min_x = self.state.plane.w_usize();
            let mut max_x = 0;
            let mut min_y = self.state.plane.h_usize();
            let mut max_y = 0;

            for i in cluster {
                let coord = points[i as usize];
                min_x = min_x.min(coord[0] as usize);
                max_x = max_x.max(coord[0] as usize);
                min_y = min_y.min(coord[1] as usize);
                max_y = max_y.max(coord[1] as usize);

                if self.state.show_features != ShowFeatureMode::Off {
                    draw_feature_coord(
                        points[i as usize][0] as PixelAddress,
                        points[i as usize][1] as PixelAddress,
                        &mut self.display_frame_features,
                        self.state.plane.c() != 1,
                        Some(random_color),
                    );
                }
            }

            // If area is less then 1/4 the size of the frame, push it
            if (max_x - min_x) * (max_y - min_y) < self.state.plane.area_wh() / 4 {
                bboxes.push((min_x, min_y, max_x, max_y));

                if self.state.show_features != ShowFeatureMode::Off {
                    draw_rect(
                        min_x as PixelAddress,
                        min_y as PixelAddress,
                        max_x as PixelAddress,
                        max_y as PixelAddress,
                        &mut self.display_frame_features,
                        self.state.plane.c() != 1,
                        Some(random_color),
                    );
                }
            }
        }
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
    pub(crate) fn update_crf(&mut self, crf: u8) {
        self.encoder.options.crf = Crf::new(Some(crf), self.state.plane);
        self.encoder.sync_crf();

        let c_thresh_baseline = self.encoder.options.crf.get_parameters().c_thresh_baseline;

        for px in self.event_pixel_trees.iter_mut() {
            px.c_thresh = c_thresh_baseline;
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
        feature_c_radius: f32,
    ) {
        {
            let crf = &mut self.encoder.options.crf;

            crf.override_c_thresh_baseline(c_thresh_baseline);
            crf.override_c_thresh_max(c_thresh_max);
            crf.override_c_increase_velocity(c_increase_velocity);
            crf.override_feature_c_radius(feature_c_radius as u16); // The absolute pixel count radius
        }
        self.state.params.delta_t_max = delta_t_max_multiplier * self.state.params.ref_time;
        self.encoder.sync_crf();

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
    frame_val: u8,
    intensity: Intensity32,
    time_spanned: f32,
    buffer: &mut Vec<Event>,
    params: &VideoStateParams,
    parameters: &CrfParameters,
) -> bool {
    let _start_len = buffer.len();
    let mut grew_buffer = false;
    if px.need_to_pop_top {
        buffer.push(px.pop_top_event(intensity, params.pixel_tree_mode, params.ref_time));
        grew_buffer = true;
    }

    *base_val = px.base_val;

    if frame_val < base_val.saturating_sub(px.c_thresh)
        || frame_val > base_val.saturating_add(px.c_thresh)
    {
        let _tmp = buffer.len();
        px.pop_best_events(
            buffer,
            params.pixel_tree_mode,
            params.pixel_multi_mode,
            params.ref_time,
            intensity,
        );
        grew_buffer = true;
        px.base_val = frame_val;

        // If continuous mode and the D value needs to be different now
        if let Continuous = params.pixel_tree_mode {
            match px.set_d_for_continuous(intensity, params.ref_time) {
                None => {}
                Some(event) => buffer.push(event),
            };
        }
    }

    px.integrate(
        intensity,
        time_spanned,
        params.pixel_tree_mode,
        params.delta_t_max,
        params.ref_time,
        parameters.c_thresh_max,
        parameters.c_increase_velocity,
        params.pixel_multi_mode,
    );

    if px.need_to_pop_top {
        buffer.push(px.pop_top_event(intensity, params.pixel_tree_mode, params.ref_time));
        grew_buffer = true;
    }

    // if buffer.len() - start_len > 5 {
    //     dbg!("hm", buffer.len() - start_len);
    // }
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

// fn convert_u8_to_f32_simd(input: &[u8]) -> Vec<f32> {
//     // Ensure that the input length is a multiple of 16
//     let len = input.len() / 16 * 16;
//
//     // Use the simd crate to load u8x16 vectors and convert to f32x4 vectors
//     let mut result: Vec<f32> = Vec::with_capacity(len / 4);
//     for i in (0..len).step_by(16) {
//         let u8_slice = &input[i..i + 16];
//         let u8x16_vector: u8x16 = u8_slice.load_unaligned().into();
//         let f32x4_vector: f32x4 = unsafe { std::mem::transmute(u8x16_vector) };
//         for j in 0..4 {
//             result.push(f32x4_vector.extract(j));
//         }
//     }
//
//     result
// }
