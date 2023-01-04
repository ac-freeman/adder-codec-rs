use opencv::core::{Mat, Size, CV_8U, CV_8UC3};
use std::error::Error;
use std::fmt;

use bumpalo::Bump;
use std::path::Path;
use std::sync::mpsc::{channel, Receiver, Sender};

use crate::raw::stream::{Error as StreamError, Raw};
use crate::{raw, Codec, Coord, Event, PlaneSize, SourceType, D};
use opencv::highgui;
use opencv::imgproc::resize;
use opencv::prelude::*;

use crate::framer::scale_intensity::FrameValue;
use crate::transcoder::d_controller::DecimationMode;
use crate::transcoder::event_pixel_tree::Mode::Continuous;
use crate::transcoder::event_pixel_tree::{DeltaT, Intensity32, Mode, PixelArena};
use crate::SourceCamera;
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

    StreamError(raw::stream::Error),

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
impl From<StreamError> for SourceError {
    fn from(value: StreamError) -> Self {
        SourceError::StreamError(value)
    }
}

#[derive(PartialEq, Eq, Clone, Copy)]
pub enum FramedViewMode {
    Intensity,
    D,
    DeltaT,
}

/// Attributes common to ADΔER transcode process
pub struct Video {
    pub plane: PlaneSize,
    pub chunk_rows: usize,
    pub(crate) event_pixel_trees: Array3<PixelArena>,
    pub(crate) ref_time: u32,
    pub(crate) ref_time_divisor: f64,
    pub(crate) delta_t_max: u32,
    pub(crate) show_display: bool,
    pub(crate) show_live: bool,
    pub in_interval_count: u32,
    pub(crate) _instantaneous_display_frame: Mat,
    pub instantaneous_frame: Mat,
    pub instantaneous_view_mode: FramedViewMode,
    pub event_sender: Sender<Vec<Event>>,
    pub(crate) write_out: bool,
    pub(crate) c_thresh_pos: u8,
    pub(crate) c_thresh_neg: u8,
    pub(crate) tps: DeltaT,
    pub(crate) stream: Raw,
}

impl Video {
    /// Initialize the Video. `width` and `height` are determined by the calling source.
    /// Also spawns a thread with an [`OutputWriter`]. This thread receives messages with [`Event`]
    /// types which are then written to the output event stream file.
    /// # Errors
    /// Returns an error if the output file cannot be opened, or if input parameters are invalid.
    pub fn new(
        plane: PlaneSize,
        chunk_rows: usize,
        output_filename: Option<String>,
        tps: DeltaT,
        ref_time: DeltaT,
        delta_t_max: DeltaT,
        _d_mode: DecimationMode,
        write_out: bool,
        show_display: bool,
        source_camera: SourceCamera,
        c_thresh_pos: u8,
        c_thresh_neg: u8,
    ) -> Result<Video, Box<dyn Error>> {
        let (event_sender, _event_receiver): (Sender<Vec<Event>>, Receiver<Vec<Event>>) = channel();
        if ref_time > f32::MAX as u32 {
            return Err("Reference time is too large".into());
        }

        let mut stream: Raw = Codec::new();
        match output_filename {
            None => {}
            Some(name) => {
                if write_out {
                    let path = Path::new(&name);
                    stream.open_writer(path)?;
                    stream.encode_header(
                        plane.clone(),
                        tps,
                        ref_time,
                        delta_t_max,
                        1,
                        source_camera,
                    )?;
                }
            }
        }

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
        let _motion_frame_mat = instantaneous_frame.clone();

        Ok(Video {
            plane,
            chunk_rows,
            event_pixel_trees,
            ref_time,
            ref_time_divisor: 1.0,
            delta_t_max,
            show_display,
            show_live: false,
            in_interval_count: 0,
            _instantaneous_display_frame: Mat::default(),
            instantaneous_frame,
            instantaneous_view_mode: FramedViewMode::Intensity,
            event_sender,
            write_out,
            stream,
            c_thresh_pos,
            c_thresh_neg,
            tps,
        })
    }

    /// Close and flush the stream writer.
    /// # Errors
    /// Returns an error if the stream writer cannot be closed cleanly.
    pub fn end_write_stream(&mut self) -> Result<(), Box<dyn Error>> {
        self.stream.close_writer()
    }

    #[allow(clippy::needless_pass_by_value)]
    pub(crate) fn integrate_matrix(
        &mut self,
        matrix: Mat,
        time_spanned: f32,
        pixel_tree_mode: Mode,
        view_interval: u32,
    ) -> std::result::Result<Vec<Vec<Event>>, SourceError> {
        let frame_arr: &[u8] = match matrix.data_bytes() {
            Ok(v) => v,
            Err(e) => {
                return Err(SourceError::OpencvError(e));
            }
        };
        if self.in_interval_count == 0 {
            self.set_initial_d(frame_arr);
        }

        self.in_interval_count += 1;

        if self.in_interval_count % view_interval == 0 {
            self.show_live = true;
        } else {
            self.show_live = false;
        }

        let px_per_chunk: usize = self.chunk_rows * self.plane.area_wc();

        // Important: if framing the events simultaneously, then the chunk division must be
        // exactly the same as it is for the framer
        let big_buffer: Vec<Vec<Event>> = self
            .event_pixel_trees
            .axis_chunks_iter_mut(Axis(0), self.chunk_rows)
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

                    *frame_val_intensity32 =
                        (f64::from(frame_arr[*px_idx]) * self.ref_time_divisor) as Intensity32;
                    *frame_val = *frame_val_intensity32 as u8;

                    integrate_for_px(
                        px,
                        base_val,
                        frame_val,
                        *frame_val_intensity32, // In this case, frame val is the same as intensity to integrate
                        time_spanned,
                        pixel_tree_mode,
                        &mut buffer,
                        &self.c_thresh_pos,
                        &self.c_thresh_neg,
                        &self.delta_t_max,
                        &self.ref_time,
                    );
                }
                buffer
            })
            .collect();

        if self.write_out {
            self.stream.encode_events_events(&big_buffer)?;
        }

        let db = match self.instantaneous_frame.data_bytes_mut() {
            Ok(v) => v,
            Err(e) => {
                return Err(SourceError::OpencvError(e));
            }
        };

        // TODO: When there's full support for various bit-depth sources, modify this accordingly
        let practical_d_max =
            fast_math::log2_raw(255.0 * (self.delta_t_max / self.ref_time) as f32);
        db.par_iter_mut().enumerate().for_each(|(idx, val)| {
            let y = idx / self.plane.area_wc();
            let x = (idx % self.plane.area_wc()) / self.plane.c_usize();
            let c = idx % self.plane.c_usize();
            *val = match self.event_pixel_trees[[y, x, c]].arena[0].best_event {
                Some(event) => u8::get_frame_value(
                    &event,
                    SourceType::U8,
                    self.ref_time as DeltaT,
                    practical_d_max,
                    self.delta_t_max,
                    self.instantaneous_view_mode,
                ),
                None => *val,
            };
        });

        if self.show_live {
            show_display("instance", &self.instantaneous_frame, 1, self)?;
        }

        Ok(big_buffer)
    }

    fn set_initial_d(&mut self, frame_arr: &[u8]) {
        self.event_pixel_trees.par_map_inplace(|px| {
            let idx = px.coord.y as usize * self.plane.area_wc()
                + px.coord.x as usize * self.plane.c_usize()
                + px.coord.c.unwrap_or(0) as usize;
            let intensity = frame_arr[idx];
            let d_start = f32::from(intensity).log2().floor() as D;
            px.arena[0].set_d(d_start);
            px.base_val = intensity;
        });
    }

    /// Get `ref_time`
    pub fn get_ref_time(&self) -> u32 {
        self.ref_time
    }

    /// Get `delta_t_max`
    pub fn get_delta_t_max(&self) -> u32 {
        self.delta_t_max
    }

    /// Get `tps`
    pub fn get_tps(&self) -> u32 {
        self.tps
    }

    /// Set a new value for `delta_t_max`
    pub fn update_delta_t_max(&mut self, dtm: u32) {
        // Validate new value
        self.delta_t_max = self.ref_time.max(dtm);
    }

    /// Set a new value for `c_thresh_pos`
    pub fn update_adder_thresh_pos(&mut self, c: u8) {
        self.c_thresh_pos = c;
    }

    /// Set a new value for `c_thresh_neg`
    pub fn update_adder_thresh_neg(&mut self, c: u8) {
        self.c_thresh_neg = c;
    }
}

pub fn integrate_for_px(
    px: &mut PixelArena,
    base_val: &mut u8,
    frame_val: &u8,
    intensity: Intensity32,
    time_spanned: f32,
    pixel_tree_mode: Mode,
    buffer: &mut Vec<Event>,
    c_thresh_pos: &u8,
    c_thresh_neg: &u8,
    delta_t_max: &u32,
    ref_time: &u32,
) {
    if px.need_to_pop_top {
        buffer.push(px.pop_top_event(intensity));
    }

    *base_val = px.base_val;

    if *frame_val < base_val.saturating_sub(*c_thresh_neg)
        || *frame_val > base_val.saturating_add(*c_thresh_pos)
    {
        px.pop_best_events(buffer);
        px.base_val = *frame_val;

        // If continuous mode and the D value needs to be different now
        if let Continuous = pixel_tree_mode {
            match px.set_d_for_continuous(intensity) {
                None => {}
                Some(event) => buffer.push(event),
            };
        }
    }

    px.integrate(
        intensity,
        time_spanned,
        pixel_tree_mode,
        *delta_t_max,
        *ref_time,
    );

    if px.need_to_pop_top {
        buffer.push(px.pop_top_event(intensity));
    }
}

/// If `video.show_display`, shows the given [`Mat`] in an `OpenCV` window
/// with the given name.
///
/// # Errors
/// Returns an [`OpencvError`] if the window cannot be shown, or the [`Mat`] cannot be scaled as
/// needed.
pub fn show_display(window_name: &str, mat: &Mat, wait: i32, video: &Video) -> opencv::Result<()> {
    if video.show_display {
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

pub trait Source {
    /// Intake one input interval worth of data from the source stream into the ADΔER model as
    /// intensities.
    fn consume(
        &mut self,
        view_interval: u32,
        thread_pool: &ThreadPool,
    ) -> Result<Vec<Vec<Event>>, SourceError>;

    fn get_video_mut(&mut self) -> &mut Video;

    fn get_video(&self) -> &Video;
}
