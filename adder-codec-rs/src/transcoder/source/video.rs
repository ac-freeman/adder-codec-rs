use opencv::core::{Mat, Scalar, Size, CV_32F, CV_32FC3, CV_8U, CV_8UC3};
use std::io::{sink, Write};
use std::mem::swap;

use adder_codec_core::codec::empty::stream::EmptyOutput;
use adder_codec_core::codec::encoder::Encoder;
use adder_codec_core::codec::raw::stream::RawOutput;
use adder_codec_core::codec::{CodecError, CodecMetadata, LATEST_CODEC_VERSION};
use adder_codec_core::{
    Coord, DeltaT, Event, Mode, PlaneError, PlaneSize, SourceCamera, SourceType, TimeMode,
};
use bumpalo::Bump;
use std::sync::mpsc::{channel, Sender};

use adder_codec_core::D;
use opencv::highgui;
use opencv::imgproc::resize;
use opencv::prelude::*;

use crate::framer::scale_intensity::{event_to_intensity, FrameValue};
use crate::transcoder::event_pixel_tree::{Intensity32, PixelArena};
use adder_codec_core::Mode::{Continuous, FramePerfect};
use davis_edi_rs::util::reconstructor::ReconstructionError;
use ndarray::{Array3, Axis, ShapeError};
use rayon::iter::IntoParallelIterator;
use rayon::iter::ParallelIterator;
use rayon::iter::{IndexedParallelIterator, IntoParallelRefMutIterator};
use rayon::ThreadPool;

use thiserror::Error;
use tokio::task::JoinError;

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

    /// OpenCV error
    #[error("OpenCV error")]
    OpencvError(opencv::Error),

    /// Codec error
    #[error("Codec core error")]
    CodecError(CodecError),

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
}

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
pub struct VideoState {
    /// The size of the imaging plane
    pub plane: PlaneSize,
    pub(crate) pixel_tree_mode: Mode,

    /// The number of rows of pixels to process at a time (per thread)
    pub chunk_rows: usize,

    /// The number of input intervals (of fixed time) processed so far
    pub in_interval_count: u32,
    pub(crate) c_thresh_pos: u8,
    pub(crate) c_thresh_neg: u8,
    pub(crate) delta_t_max: u32,
    pub(crate) ref_time: u32,
    pub(crate) ref_time_divisor: f64,
    pub(crate) tps: DeltaT,
    pub(crate) show_display: bool,
    pub(crate) show_live: bool,
}

impl Default for VideoState {
    fn default() -> Self {
        VideoState {
            plane: PlaneSize::default(),
            pixel_tree_mode: Continuous,
            chunk_rows: 64,
            in_interval_count: 1,
            c_thresh_pos: 0,
            c_thresh_neg: 0,
            delta_t_max: 7650,
            ref_time: 255,
            ref_time_divisor: 1.0,
            tps: 7650,
            show_display: false,
            show_live: false,
        }
    }
}

/// A builder for a [`Video`]
pub trait VideoBuilder<W> {
    /// Set both the positive and negative contrast thresholds
    fn contrast_thresholds(self, c_thresh_pos: u8, c_thresh_neg: u8) -> Self;

    /// Set the positive contrast threshold
    fn c_thresh_pos(self, c_thresh_pos: u8) -> Self;

    /// Set the negative contrast threshold
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
        write: W,
    ) -> Result<Box<Self>, SourceError>;

    /// Set whether or not the show the live display
    fn show_display(self, show_display: bool) -> Self;
}

// impl VideoBuilder for Video {}

/// Attributes common to ADΔER transcode process
pub struct Video<W: Write> {
    /// The current state of the video transcode
    pub state: VideoState,
    pub(crate) event_pixel_trees: Array3<PixelArena>,

    /// The current instantaneous frame
    pub instantaneous_frame: Mat,

    abs_intensity_mat: Mat,

    /// The current view mode of the instantaneous frame
    pub instantaneous_view_mode: FramedViewMode,

    /// Channel for sending events to the encoder
    pub event_sender: Sender<Vec<Event>>,
    pub(crate) encoder: Encoder<W>,
    // TODO: Hold multiple encoder options and an enum, so that boxing isn't required.
    // Also hold a state for whether or not to write out events at all, so that a null writer isn't required.
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
        let mut instantaneous_frame = Mat::default();
        match plane.c() {
            1 => unsafe {
                instantaneous_frame.create_rows_cols(plane.h() as i32, plane.w() as i32, CV_8U)?;
            },
            _ => unsafe {
                instantaneous_frame.create_rows_cols(
                    plane.h() as i32,
                    plane.w() as i32,
                    CV_8UC3,
                )?;
            },
        }

        let mut sae_mat = Mat::default();
        match plane.c() {
            1 => unsafe {
                sae_mat.create_rows_cols(plane.h() as i32, plane.w() as i32, CV_32F)?;
            },
            _ => unsafe {
                sae_mat.create_rows_cols(plane.h() as i32, plane.w() as i32, CV_32FC3)?;
            },
        }
        let mut abs_intensity_mat = sae_mat.clone();

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
                    instantaneous_frame,
                    abs_intensity_mat,
                    instantaneous_view_mode,
                    event_sender,
                    encoder,
                })
            }
            Some(w) => {
                let encoder = Encoder::new_raw(
                    // TODO: Allow for compressed representation (not just raw)
                    RawOutput::new(meta, w),
                );
                Ok(Video {
                    state,
                    event_pixel_trees,
                    instantaneous_frame,
                    abs_intensity_mat,
                    instantaneous_view_mode,
                    event_sender,
                    encoder,
                })
            }
        }
    }

    /// Set the positive contrast threshold
    pub fn c_thresh_pos(mut self, c_thresh_pos: u8) -> Self {
        self.state.c_thresh_pos = c_thresh_pos;
        self
    }

    /// Set the negative contrast threshold
    pub fn c_thresh_neg(mut self, c_thresh_neg: u8) -> Self {
        self.state.c_thresh_neg = c_thresh_neg;
        self
    }

    /// Set the number of rows to process at a time (in each thread)
    pub fn chunk_rows(mut self, chunk_rows: usize) -> Self {
        self.state.chunk_rows = chunk_rows;
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
        write: W,
    ) -> Result<Self, SourceError> {
        // TODO: Allow for compressed representation (not just raw)
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
        let encoder: Encoder<_> = Encoder::new_raw(compression);
        self.encoder = encoder;

        dbg!(time_mode);
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
        matrix: Mat,
        time_spanned: f32,
        view_interval: u32,
    ) -> std::result::Result<Vec<Vec<Event>>, SourceError> {
        let frame_arr: &[u8] = match matrix.data_bytes() {
            Ok(v) => v,
            Err(e) => {
                return Err(SourceError::OpencvError(e));
            }
        };
        if self.state.in_interval_count == 0 {
            self.set_initial_d(frame_arr);
        }

        self.state.in_interval_count += 1;

        if self.state.in_interval_count % view_interval == 0 {
            self.state.show_live = true;
        } else {
            self.state.show_live = false;
        }

        let px_per_chunk: usize = self.state.chunk_rows * self.state.plane.area_wc();

        // Important: if framing the events simultaneously, then the chunk division must be
        // exactly the same as it is for the framer
        let big_buffer: Vec<Vec<Event>> = self
            .event_pixel_trees
            .axis_chunks_iter_mut(Axis(0), self.state.chunk_rows)
            .into_par_iter()
            .enumerate()
            .map(|(chunk_idx, mut chunk)| {
                let mut buffer: Vec<Event> = Vec::with_capacity(px_per_chunk);
                let bump = Bump::new();
                let base_val = bump.alloc(0);
                let px_idx = bump.alloc(0);
                let frame_val = bump.alloc(0);
                let frame_val_intensity32 = bump.alloc(0.0);

                for (chunk_px_idx, px) in chunk.iter_mut().enumerate() {
                    *px_idx = chunk_px_idx + px_per_chunk * chunk_idx;

                    *frame_val_intensity32 = (f64::from(frame_arr[*px_idx])
                        * self.state.ref_time_divisor)
                        as Intensity32;
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

        self.encoder.ingest_events_events(&big_buffer)?;

        let db = match self.instantaneous_frame.data_bytes_mut() {
            Ok(v) => v,
            Err(e) => {
                return Err(SourceError::OpencvError(e));
            }
        };
        // let mut sae_mat = Mat::default();
        // match self.state.plane.c() {
        //     1 => unsafe {
        //         sae_mat.create_rows_cols(
        //             self.state.plane.h() as i32,
        //             self.state.plane.w() as i32,
        //             CV_32F,
        //         )?;
        //     },
        //     _ => unsafe {
        //         sae_mat.create_rows_cols(
        //             self.state.plane.h() as i32,
        //             self.state.plane.w() as i32,
        //             CV_32FC3,
        //         )?;
        //     },
        // }
        // sae_mat = sae_mat.clone();
        //
        // for events in &big_buffer {
        //     for event in events {
        //         let y = event.coord.y;
        //         let x = event.coord.x;
        //
        //         *self
        //             .abs_intensity_mat
        //             .at_2d_mut::<f32>(y as i32, x as i32)
        //             .unwrap() = event_to_intensity(&event) as f32;
        //     }
        // }

        // TODO: When there's full support for various bit-depth sources, modify this accordingly
        let practical_d_max =
            fast_math::log2_raw(255.0 * (self.state.delta_t_max / self.state.ref_time) as f32);
        db.par_iter_mut().enumerate().for_each(|(idx, val)| {
            let y = idx / self.state.plane.area_wc();
            let x = (idx % self.state.plane.area_wc()) / self.state.plane.c_usize();
            let c = idx % self.state.plane.c_usize();

            let sae_time_since = self.event_pixel_trees[[y, x, c]].running_t
                - self.event_pixel_trees[[y, x, c]].last_fired_t;
            *val = match self.event_pixel_trees[[y, x, c]].arena[0].best_event {
                Some(event) => u8::get_frame_value(
                    &event.into(),
                    SourceType::U8,
                    self.state.ref_time as DeltaT,
                    practical_d_max,
                    self.state.delta_t_max,
                    self.instantaneous_view_mode,
                    sae_time_since,
                ),
                None => *val,
            };

            // if self.instantaneous_view_mode == FramedViewMode::SAE {
            //     // let tmp = sae_mat.at_2d::<f32>(y as i32, x as i32).unwrap();
            //     unsafe {
            //         *sae_mat.at_2d_mut::<f32>(y as i32, x as i32).unwrap() = sae_time_since as f32;
            //     }
            // }
        });

        // let mut sae_mat_norm = Mat::default();
        // opencv::core::normalize(
        //     &sae_mat,
        //     &mut sae_mat_norm,
        //     0.0,
        //     255.0,
        //     opencv::core::NORM_MINMAX,
        //     opencv::core::CV_8U,
        //     &Mat::default(),
        // )?;
        // // subtract each element from 255
        // opencv::core::subtract(
        //     &Scalar::new(255.0, 255.0, 255.0, 0.0),
        //     &sae_mat_norm.clone(),
        //     &mut sae_mat_norm,
        //     &Mat::default(),
        //     opencv::core::CV_8U,
        // )?;

        // show_display_force("abs", &self.abs_intensity_mat, 0)?;

        // let mut abs_intensity_mat_norm = Mat::default();
        // opencv::core::normalize(
        //     &self.abs_intensity_mat,
        //     &mut abs_intensity_mat_norm,
        //     0.0,
        //     255.0,
        //     opencv::core::NORM_MINMAX,
        //     opencv::core::CV_8U,
        //     &Mat::default(),
        // )?;
        // opencv::core::subtract(
        //     &Scalar::new(255.0, 255.0, 255.0, 0.0),
        //     &abs_intensity_mat_norm.clone(),
        //     &mut abs_intensity_mat_norm,
        //     &Mat::default(),
        //     opencv::core::CV_8U,
        // )?;
        // opencv::core::normalize(
        //     &sae_mat_norm.clone(),
        //     &mut sae_mat_norm,
        //     0.0,
        //     255.0,
        //     opencv::core::NORM_L1,
        //     opencv::core::CV_8U,
        //     &Mat::default(),
        // )?;
        // self.instantaneous_frame = sae_mat_norm;
        // self.instantaneous_frame = abs_intensity_mat_norm;

        if self.instantaneous_view_mode == FramedViewMode::DeltaT {
            opencv::core::normalize(
                &self.instantaneous_frame.clone(),
                &mut self.instantaneous_frame,
                0.0,
                255.0,
                opencv::core::NORM_MINMAX,
                opencv::core::CV_8U,
                &Mat::default(),
            )?;
            opencv::core::subtract(
                &Scalar::new(255.0, 255.0, 255.0, 0.0),
                &self.instantaneous_frame.clone(),
                &mut self.instantaneous_frame,
                &Mat::default(),
                opencv::core::CV_8U,
            )?;
        }

        // let mut corners = Mat::default();
        //
        // opencv::imgproc::corner_harris(
        //     &self.instantaneous_frame.clone(),
        //     &mut corners,
        //     5,
        //     3,
        //     0.04,
        //     opencv::core::BORDER_DEFAULT,
        // )?;
        // opencv::core::normalize(
        //     &corners.clone(),
        //     &mut corners,
        //     0.0,
        //     255.0,
        //     opencv::core::NORM_MINMAX,
        //     opencv::core::CV_8U,
        //     &Mat::default(),
        // )?;
        //
        // show_display_force("corners", &corners, 1)?;

        if self.state.show_live {
            show_display("instance", &self.instantaneous_frame, 1, self)?;
        }

        Ok(big_buffer)
    }

    fn set_initial_d(&mut self, frame_arr: &[u8]) {
        self.event_pixel_trees.par_map_inplace(|px| {
            let idx = px.coord.y as usize * self.state.plane.area_wc()
                + px.coord.x as usize * self.state.plane.c_usize()
                + px.coord.c.unwrap_or(0) as usize;
            let intensity = frame_arr[idx];
            let d_start = f32::from(intensity).log2().floor() as D;
            px.arena[0].set_d(d_start);
            px.base_val = intensity;
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

    /// Set a new value for `c_thresh_pos`
    pub fn update_adder_thresh_pos(&mut self, c: u8) {
        self.state.c_thresh_pos = c;
    }

    /// Set a new value for `c_thresh_neg`
    pub fn update_adder_thresh_neg(&mut self, c: u8) {
        self.state.c_thresh_neg = c;
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
pub fn integrate_for_px(
    px: &mut PixelArena,
    base_val: &mut u8,
    frame_val: &u8,
    intensity: Intensity32,
    time_spanned: f32,
    buffer: &mut Vec<Event>,
    state: &VideoState,
) {
    if px.need_to_pop_top {
        buffer.push(px.pop_top_event(intensity, state.pixel_tree_mode, state.ref_time));
    }

    *base_val = px.base_val;

    if *frame_val < base_val.saturating_sub(state.c_thresh_neg)
        || *frame_val > base_val.saturating_add(state.c_thresh_pos)
    {
        px.pop_best_events(buffer, state.pixel_tree_mode, state.ref_time);
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
    );

    if px.need_to_pop_top {
        buffer.push(px.pop_top_event(intensity, state.pixel_tree_mode, state.ref_time));
    }
}

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

    /// Get a mutable reference to the [`Video`] object associated with this [`Source`].
    fn get_video_mut(&mut self) -> &mut Video<W>;

    /// Get an immutable reference to the [`Video`] object associated with this [`Source`].
    fn get_video_ref(&self) -> &Video<W>;

    /// Get the [`Video`] object associated with this [`Source`], consuming the [`Source`] in the
    /// process.
    fn get_video(self) -> Video<W>;
}
