use opencv::core::{Mat, Size, CV_8U, CV_8UC3};

use std::path::Path;
use std::sync::mpsc::{channel, Receiver, Sender};

use crate::raw::raw_stream::RawStream;
use crate::{Codec, Coord, Event, D_MAX, D_SHIFT};
use opencv::highgui;
use opencv::imgproc::resize;
use opencv::prelude::*;

use crate::transcoder::d_controller::DecimationMode;
use crate::transcoder::event_pixel_tree::{DeltaT, PixelArena};
use crate::SourceCamera;
use ndarray::Array3;

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
    pub(crate) event_pixel_trees: Array3<PixelArena>,
    pub(crate) ref_time: u32,
    pub(crate) delta_t_max: u32,
    pub(crate) show_display: bool,
    pub(crate) show_live: bool,
    pub in_interval_count: u32,
    pub(crate) instantaneous_display_frame: Mat,
    pub(crate) instantaneous_frame: Mat,
    pub event_sender: Sender<Vec<Event>>,
    pub(crate) write_out: bool,
    pub channels: usize,
    pub(crate) stream: RawStream,
}

impl Video {
    /// Initialize the Video. `width` and `height` are determined by the calling source.
    /// Also spawns a thread with an [`OutputWriter`]. This thread receives messages with [`Event`]
    /// types which are then written to the output event stream file.
    pub fn new(
        width: u16,
        height: u16,
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
        let motion_frame_mat = instantaneous_frame.clone();

        Video {
            width,
            height,
            event_pixel_trees,
            ref_time,
            delta_t_max,
            show_display,
            show_live: false,
            in_interval_count: 0,
            instantaneous_display_frame: Mat::default(),
            instantaneous_frame,
            event_sender,
            write_out,
            channels,
            stream,
        }
    }

    pub fn end_write_stream(&mut self) {
        self.stream.close_writer();
    }
}

/// If [`MyArgs`]`.show_display`, shows the given [`Mat`] in an OpenCV window
pub fn show_display(window_name: &str, mat: &Mat, wait: i32, video: &Video) {
    if video.show_display {
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
}

pub trait Source {
    /// Intake one input interval worth of data from the source stream into the ADΔER model as
    /// intensities
    fn consume(&mut self, view_interval: u32) -> Result<Vec<Vec<Event>>, SourceError>;

    fn get_video_mut(&mut self) -> &mut Video;

    fn get_video(&self) -> &Video;
}
