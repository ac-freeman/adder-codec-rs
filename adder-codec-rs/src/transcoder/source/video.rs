use adder_codec_core::codec::raw::stream::Raw;
use opencv::core::{Mat, Size, CV_8U, CV_8UC3};
use std::error::Error;
use std::fmt;
use std::fs::File;
use std::io::{BufWriter, Seek, Write};

use adder_codec_core::codec::encoder::Encoder;
use adder_codec_core::codec::{CodecError, CodecMetadata, WriteCompression, LATEST_CODEC_VERSION};
use adder_codec_core::{PlaneSize, SourceCamera, TimeMode};
use bitstream_io::BitWrite;
use bumpalo::Bump;
use std::path::Path;
use std::sync::mpsc::{channel, Sender};

use crate::{codec, Coord, Event, SourceType, D};
use opencv::highgui;
use opencv::imgproc::resize;
use opencv::prelude::*;

use crate::framer::scale_intensity::FrameValue;
use crate::transcoder::event_pixel_tree::Mode::Continuous;
use crate::transcoder::event_pixel_tree::{DeltaT, Intensity32, Mode, PixelArena};
use davis_edi_rs::util::reconstructor::ReconstructionError;
use ndarray::{Array3, Axis};
use rayon::iter::IntoParallelIterator;
use rayon::iter::ParallelIterator;
use rayon::iter::{IndexedParallelIterator, IntoParallelRefMutIterator};
use rayon::ThreadPool;

#[derive(Debug)]
pub enum SourceError {
    /// Could not open source file
    Open,

    /// ADDER parameters are invalid for the given source
    BadParams,

    StartOutOfBounds,

    /// Source buffer is empty
    BufferEmpty,

    /// Source buffer channel is closed
    BufferChannelClosed,

    /// No data from next spot in buffer
    NoData,

    /// Data not initialized
    UninitializedData,

    /// OpenCV error
    OpencvError(opencv::Error),

    StreamError(CodecError),

    /// EDI error
    EdiError(ReconstructionError),
}

impl fmt::Display for SourceError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Source error")
    }
}

impl From<SourceError> for Box<dyn std::error::Error> {
    fn from(value: SourceError) -> Self {
        value.to_string().into()
    }
}

impl From<opencv::Error> for SourceError {
    fn from(value: opencv::Error) -> Self {
        SourceError::OpencvError(value)
    }
}
impl From<CodecError> for SourceError {
    fn from(value: CodecError) -> Self {
        SourceError::CodecError(value)
    }
}

#[derive(PartialEq, Eq, Clone, Copy, Debug)]
pub enum FramedViewMode {
    Intensity,
    D,
    DeltaT,
}

pub struct VideoState {
    pub plane: PlaneSize,
    pub(crate) pixel_tree_mode: Mode,
    pub chunk_rows: usize,
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
            pixel_tree_mode: Mode::Continuous,
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

pub trait VideoBuilder<W> {
    fn contrast_thresholds(self, c_thresh_pos: u8, c_thresh_neg: u8) -> Self;

    fn c_thresh_pos(self, c_thresh_pos: u8) -> Self;

    fn c_thresh_neg(self, c_thresh_neg: u8) -> Self;

    fn chunk_rows(self, chunk_rows: usize) -> Self;

    fn time_parameters(
        self,
        tps: DeltaT,
        ref_time: DeltaT,
        delta_t_max: DeltaT,
    ) -> Result<Self, Box<dyn Error>>
    where
        Self: std::marker::Sized;

    fn write_out(
        self,
        output_filename: String,
        source_camera: SourceCamera,
        time_mode: TimeMode,
        write: W,
    ) -> Result<Box<Self>, Box<dyn std::error::Error>>;

    fn show_display(self, show_display: bool) -> Self;
}

// impl VideoBuilder for Video {}

/// Attributes common to ADΔER transcode process
pub struct Video<W: Write> {
    pub state: VideoState,
    pub(crate) event_pixel_trees: Array3<PixelArena>,
    pub instantaneous_frame: Mat,
    pub instantaneous_view_mode: FramedViewMode,
    pub event_sender: Sender<Vec<Event>>,
    pub(crate) encoder: Encoder<W>,
}

unsafe impl<W: Write> Send for Video<W> {}

impl<W: Write> Video<W> {
    /// Initialize the Video with default parameters.
    pub(crate) fn new(
        plane: PlaneSize,
        pixel_tree_mode: Mode,
        writer: Option<W>,
    ) -> Result<Video<W>, Box<dyn Error>> {
        let mut state = VideoState {
            pixel_tree_mode,
            ..Default::default()
        };

        let mut data = Vec::new();
        for y in 0..plane.height {
            for x in 0..plane.width {
                for c in 0..plane.channels {
                    let px = PixelArena::new(
                        1.0,
                        Coord {
                            x,
                            y,
                            c: match &plane.channels {
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
        match plane.channels {
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

        state.plane = plane;
        let instantaneous_view_mode = FramedViewMode::Intensity;
        let (event_sender, _) = channel();

        Ok(Video {
            state,
            event_pixel_trees,
            instantaneous_frame,
            instantaneous_view_mode,
            event_sender,
            stream: None,
            writer,
        })
    }

    pub fn c_thresh_pos(mut self, c_thresh_pos: u8) -> Self {
        self.state.c_thresh_pos = c_thresh_pos;
        self
    }

    pub fn c_thresh_neg(mut self, c_thresh_neg: u8) -> Self {
        self.state.c_thresh_neg = c_thresh_neg;
        self
    }

    pub fn chunk_rows(mut self, chunk_rows: usize) -> Self {
        self.state.chunk_rows = chunk_rows;
        self
    }

    pub fn time_parameters(
        mut self,
        tps: DeltaT,
        ref_time: DeltaT,
        delta_t_max: DeltaT,
    ) -> Result<Self, Box<dyn Error>> {
        if self.stream.is_some() {
            return Err(
                "Cannot change time parameters after output stream has been initialized".into(),
            );
        }
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

    pub fn write_out(
        mut self,
        output_filename: String,
        source_camera: Option<SourceCamera>,
        time_mode: Option<TimeMode>,
        write: W,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let compression = Raw::new(
            CodecMetadata {
                codec_version: LATEST_CODEC_VERSION,
                header_size: 0,
                time_mode: time_mode.unwrap_or_default(),
                plane: self.state.plane.clone(),
                tps: self.state.tps,
                ref_interval: self.state.ref_time,
                delta_t_max: self.state.delta_t_max,
                event_size: 0,
                source_camera: source_camera,
            },
            write,
        );
        let mut encoder: Encoder<BufWriter<Vec<u8>>> = Encoder::new(Box::new(compression));
        self.encoder = encoder;

        self.event_pixel_trees.par_map_inplace(|px| {
            px.time_mode(time_mode);
        });
        Ok(self)
    }

    pub fn show_display(mut self, show_display: bool) -> Self {
        self.state.show_display = show_display;
        self
    }

    /// Close and flush the stream writer.
    /// # Errors
    /// Returns an error if the stream writer cannot be closed cleanly.
    pub fn end_write_stream(&mut self) -> Result<(), Box<dyn Error>> {
        match &mut self.stream {
            Some(s) => s.close_writer(),
            None => Ok(()),
        }
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

        if let Some(stream) = &mut self.stream {
            stream.encode_events_events(&big_buffer)?;
        }

        let db = match self.instantaneous_frame.data_bytes_mut() {
            Ok(v) => v,
            Err(e) => {
                return Err(SourceError::OpencvError(e));
            }
        };

        // TODO: When there's full support for various bit-depth sources, modify this accordingly
        let practical_d_max =
            fast_math::log2_raw(255.0 * (self.state.delta_t_max / self.state.ref_time) as f32);
        db.par_iter_mut().enumerate().for_each(|(idx, val)| {
            let y = idx / self.state.plane.area_wc();
            let x = (idx % self.state.plane.area_wc()) / self.state.plane.c_usize();
            let c = idx % self.state.plane.c_usize();
            *val = match self.event_pixel_trees[[y, x, c]].arena[0].best_event {
                Some(event) => u8::get_frame_value(
                    &event.into(),
                    SourceType::U8,
                    self.state.ref_time as DeltaT,
                    practical_d_max,
                    self.state.delta_t_max,
                    self.instantaneous_view_mode,
                ),
                None => *val,
            };
        });

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
            match px.set_d_for_continuous(intensity) {
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
/// Returns an [`OpencvError`] if the window cannot be shown, or the [`Mat`] cannot be scaled as
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
/// Returns an [`OpencvError`] if the window cannot be shown, or the [`Mat`] cannot be scaled as
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

pub trait Source<W: Write> {
    /// Intake one input interval worth of data from the source stream into the ADΔER model as
    /// intensities.
    fn consume(
        &mut self,
        view_interval: u32,
        thread_pool: &ThreadPool,
    ) -> Result<Vec<Vec<Event>>, SourceError>;

    fn get_video_mut(&mut self) -> &mut Video<W>;

    fn get_video_ref(&self) -> &Video<W>;

    fn get_video(self) -> Video<W>;
}
