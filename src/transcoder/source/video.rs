use opencv::core::{Mat, Size, CV_8U, CV_8UC3};

use bumpalo::Bump;
use std::path::Path;
use std::sync::mpsc::{channel, Receiver, Sender};

use crate::raw::raw_stream::RawStream;
use crate::{Codec, Coord, Event, SourceType, D, D_MAX, D_SHIFT};
use opencv::highgui;
use opencv::imgproc::resize;
use opencv::prelude::*;

use crate::framer::scale_intensity::FrameValue;
use crate::transcoder::d_controller::DecimationMode;
use crate::transcoder::event_pixel_tree::Mode::Continuous;
use crate::transcoder::event_pixel_tree::{DeltaT, Intensity32, Mode, PixelArena};
use crate::SourceCamera;
use ndarray::{Array3, Axis};
use rayon::iter::IntoParallelIterator;
use rayon::iter::ParallelIterator;
use rayon::iter::{IndexedParallelIterator, IntoParallelRefMutIterator};
use rayon::ThreadPool;

#[derive(Debug)]
pub enum SourceError {
    /// Could not open source file
    Open,

    /// Source buffer is empty
    BufferEmpty,

    /// Source buffer channel is closed
    BufferChannelClosed,

    /// No data from next spot in buffer
    NoData,
}

/// Attributes common to ADΔER transcode process
pub struct Video {
    pub width: u16,
    pub height: u16,
    pub chunk_rows: usize,
    pub(crate) event_pixel_trees: Array3<PixelArena>,
    pub(crate) ref_time: u32,
    pub(crate) ref_time_divisor: f64,
    pub(crate) delta_t_max: u32,
    pub(crate) show_display: bool,
    pub(crate) show_live: bool,
    pub in_interval_count: u32,
    pub(crate) _instantaneous_display_frame: Mat,
    pub(crate) instantaneous_frame: Mat,
    pub event_sender: Sender<Vec<Event>>,
    pub(crate) write_out: bool,
    pub channels: usize,
    pub(crate) c_thresh_pos: u8,
    pub(crate) c_thresh_neg: u8,
    pub(crate) tps: DeltaT,
    pub(crate) stream: RawStream,
}

impl Video {
    /// Initialize the Video. `width` and `height` are determined by the calling source.
    /// Also spawns a thread with an [`OutputWriter`]. This thread receives messages with [`Event`]
    /// types which are then written to the output event stream file.
    pub fn new(
        width: u16,
        height: u16,
        chunk_rows: usize,
        output_filename: Option<String>,
        channels: usize,
        tps: DeltaT,
        ref_time: DeltaT,
        delta_t_max: DeltaT,
        _d_mode: DecimationMode,
        write_out: bool,
        communicate_events: bool,
        show_display: bool,
        source_camera: SourceCamera,
        c_thresh_pos: u8,
        c_thresh_neg: u8,
    ) -> Video {
        assert_eq!(D_SHIFT.len(), D_MAX as usize + 1);
        if write_out {
            assert!(communicate_events);
        }

        let (event_sender, _event_receiver): (Sender<Vec<Event>>, Receiver<Vec<Event>>) = channel();

        let mut stream: RawStream = Codec::new();
        match output_filename {
            None => {}
            Some(name) => {
                if write_out {
                    let path = Path::new(&name);
                    match stream.open_writer(path) {
                        Ok(_) => {}
                        Err(e) => {
                            panic!("{}", e)
                        }
                    };
                    stream.encode_header(
                        width,
                        height,
                        tps,
                        ref_time,
                        delta_t_max,
                        channels as u8,
                        1,
                        source_camera,
                    );
                }
            }
        }

        let mut data = Vec::new();
        for y in 0..height {
            for x in 0..width {
                for c in 0..channels {
                    let px = PixelArena::new(
                        1.0,
                        Coord {
                            x,
                            y,
                            c: match channels {
                                1 => None,
                                _ => Some(c as u8),
                            },
                        },
                    );
                    data.push(px);
                }
            }
        }

        let event_pixel_trees: Array3<PixelArena> =
            Array3::from_shape_vec((height.into(), width.into(), channels), data).unwrap();

        let mut instantaneous_frame = Mat::default();
        match channels {
            1 => unsafe {
                instantaneous_frame
                    .create_rows_cols(height as i32, width as i32, CV_8U)
                    .unwrap();
            },
            _ => unsafe {
                instantaneous_frame
                    .create_rows_cols(height as i32, width as i32, CV_8UC3)
                    .unwrap();
            },
        }
        let _motion_frame_mat = instantaneous_frame.clone();

        Video {
            width,
            height,
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
            event_sender,
            write_out,
            channels,
            stream,
            c_thresh_pos,
            c_thresh_neg,
            tps,
        }
    }

    pub fn end_write_stream(&mut self) {
        self.stream.close_writer();
    }

    pub(crate) fn integrate_matrix(
        &mut self,
        matrix: Mat,
        time_spanned: f32,
        pixel_tree_mode: Mode,
        view_interval: u32,
    ) -> std::result::Result<Vec<Vec<Event>>, SourceError> {
        let frame_arr: &[u8] = matrix.data_bytes().unwrap();
        if self.in_interval_count == 0 {
            self.set_initial_d(frame_arr);
        }

        self.in_interval_count += 1;

        if self.in_interval_count % view_interval == 0 {
            self.show_live = true;
        } else {
            self.show_live = false;
        }

        let px_per_chunk: usize = self.chunk_rows * self.width as usize * self.channels as usize;

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
                        (frame_arr[*px_idx] as f64 * self.ref_time_divisor) as Intensity32;
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
                    )
                }
                buffer
            })
            .collect();

        if self.write_out {
            self.stream.encode_events_events(&big_buffer);
        }

        show_display("Input", &matrix, 1, self);

        if self.show_live {
            let db = self.instantaneous_frame.data_bytes_mut().unwrap();
            db.par_iter_mut().enumerate().for_each(|(idx, val)| {
                let y = idx / (self.width as usize * self.channels);
                let x = (idx % (self.width as usize * self.channels)) / self.channels;
                let c = idx % self.channels;
                *val = match self.event_pixel_trees[[y, x, c]].arena[0].best_event {
                    Some(event) => {
                        u8::get_frame_value(&event, SourceType::U8, self.ref_time as DeltaT)
                    }
                    None => *val,
                };
            });

            show_display("instance", &self.instantaneous_frame, 1, self);
        }

        Ok(big_buffer)
    }

    fn set_initial_d(&mut self, frame_arr: &[u8]) {
        self.event_pixel_trees.par_map_inplace(|px| {
            let idx = px.coord.y as usize * self.width as usize * self.channels
                + px.coord.x as usize * self.channels
                + px.coord.c.unwrap_or(0) as usize;
            let intensity = frame_arr[idx];
            let d_start = (intensity as f32).log2().floor() as D;
            px.arena[0].set_d(d_start);
            px.base_val = intensity;
        });
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
        buffer.push(px.pop_top_event(Some(intensity)));
    }

    *base_val = px.base_val;

    if *frame_val < base_val.saturating_sub(*c_thresh_neg)
        || *frame_val > base_val.saturating_add(*c_thresh_pos)
    {
        px.pop_best_events(None, buffer);
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
        &pixel_tree_mode,
        delta_t_max,
        ref_time,
    );

    if px.need_to_pop_top {
        buffer.push(px.pop_top_event(Some(intensity)));
    }
}

/// If [`MyArgs`]`.show_display`, shows the given [`Mat`] in an OpenCV window
pub fn show_display(window_name: &str, mat: &Mat, wait: i32, video: &Video) {
    if video.show_display {
        show_display_force(window_name, mat, wait);
    }
}

pub fn show_display_force(window_name: &str, mat: &Mat, wait: i32) {
    let mut tmp = Mat::default();

    if mat.rows() != 940 {
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
        )
        .unwrap();
        highgui::imshow(window_name, &tmp).unwrap();
    } else {
        highgui::imshow(window_name, mat).unwrap();
    }

    // highgui::imshow(window_name, &tmp).unwrap();

    highgui::wait_key(wait).unwrap();
    // resize_window(window_name, mat.cols() / 540, 540);
}

pub trait Source {
    /// Intake one input interval worth of data from the source stream into the ADΔER model as
    /// intensities
    fn consume(
        &mut self,
        view_interval: u32,
        thread_pool: &ThreadPool,
    ) -> Result<Vec<Vec<Event>>, SourceError>;

    fn get_video_mut(&mut self) -> &mut Video;

    fn get_video(&self) -> &Video;
}

impl<'a, T> Source for &'a T
where
    T: Source,
{
    fn consume(
        &mut self,
        view_interval: u32,
        thread_pool: &ThreadPool,
    ) -> Result<Vec<Vec<Event>>, SourceError> {
        todo!()
    }

    fn get_video_mut(&mut self) -> &mut Video {
        todo!()
    }

    fn get_video(&self) -> &Video {
        todo!()
    }
}
// impl<'a, T> Source for &'a T
// where
//     T: Source,
// {
//     fn consume(
//         &mut self,
//         view_interval: u32,
//         thread_pool: &ThreadPool,
//     ) -> Result<Vec<Vec<Event>>, SourceError> {
//         println!("0");
//         (*self).consume(view_interval, thread_pool)
//     }
//
//     fn get_video_mut(&mut self) -> &mut Video {
//         (*self).get_video_mut()
//     }
//
//     fn get_video(&self) -> &Video {
//         (*self).get_video()
//     }
// }
// impl<'a, T> Source for &'a mut T
// where
//     T: Source,
// {
//     fn consume(
//         &mut self,
//         view_interval: u32,
//         thread_pool: &ThreadPool,
//     ) -> Result<Vec<Vec<Event>>, SourceError> {
//         (*self).consume(view_interval, thread_pool)
//     }
//
//     fn get_video_mut(&mut self) -> &mut Video {
//         (*self).get_video_mut()
//     }
//
//     fn get_video(&self) -> &Video {
//         (*self).get_video()
//     }
// }
