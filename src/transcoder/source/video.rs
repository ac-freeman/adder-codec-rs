use opencv::core::{
    add, no_array, normalize, Mat, Point, Size, BORDER_DEFAULT, CV_8U, CV_8UC3, NORM_MINMAX,
};

use std::path::Path;
use std::sync::mpsc::{channel, Receiver, Sender};

use crate::raw::raw_stream::RawStream;
use crate::transcoder::event_pixel::pixel::EventPixel;
use crate::transcoder::event_pixel::{DeltaT, PixelAddress};
use crate::{Codec, Event};
use opencv::imgproc::{bounding_rect, contour_area, rectangle, resize, RETR_EXTERNAL};
use opencv::{highgui, prelude::*};

use crate::SourceCamera;
use ndarray::Array3;
use ndarray::Axis;
use rayon::iter::IntoParallelIterator;
use rayon::iter::{IndexedParallelIterator, ParallelIterator};

/// Attributes common to ADΔER transcode process
pub struct Video {
    pub width: u16,
    pub height: u16,

    // NB: as of 4/15, boxing this attribute hurts performance slightly
    pub(crate) event_pixels: Array3<EventPixel>,
    pub(crate) ref_time: u32,
    pub(crate) delta_t_max: u32,
    pub(crate) show_display: bool,
    pub(crate) show_live: bool,
    pub in_interval_count: u32,
    pub(crate) instantaneous_display_frame: Mat,
    pub(crate) input_frame_8u: Mat, // TODO: only makes sense for a framed source
    pub(crate) motion_frame_mat: Mat,
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
        output_filename: String,
        channels: usize,
        tps: DeltaT,
        ref_time: DeltaT,
        delta_t_max: DeltaT,
        d_mode: u32,
        write_out: bool,
        show_display: bool,
        source_camera: SourceCamera,
    ) -> Video {
        let path = Path::new(&output_filename);

        let (event_sender, _event_receiver): (Sender<Vec<Event>>, Receiver<Vec<Event>>) = channel();

        let mut stream: RawStream = Codec::new();
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

        let mut data = Vec::new();
        for y in 0..height {
            for x in 0..width {
                for c in 0..channels {
                    let px = EventPixel::new(
                        y as PixelAddress,
                        x as PixelAddress,
                        c as u8,
                        ref_time,
                        delta_t_max,
                        d_mode,
                        channels.try_into().unwrap(),
                    );
                    data.push(px);
                }
            }
        }

        let event_pixels: Array3<EventPixel> =
            Array3::from_shape_vec((height.into(), width.into(), channels.into()), data).unwrap();

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
            event_pixels,
            ref_time,
            delta_t_max,
            show_display,
            show_live: false,
            in_interval_count: 0,
            instantaneous_display_frame: Mat::default(),
            input_frame_8u: Mat::default(),
            motion_frame_mat,
            instantaneous_frame,
            event_sender,
            write_out,
            channels,
            stream,
        }
    }

    /// Performs inter-pixel D-value adjustment
    pub fn inter_d_adjustment(&mut self, instantaneous_frame_prev: &mut Mat) {
        let mut instantaneous_frame_difference = Mat::default();
        opencv::core::subtract(
            &self.instantaneous_frame,
            instantaneous_frame_prev,
            &mut instantaneous_frame_difference,
            &opencv::core::no_array(),
            -1,
        )
        .unwrap();
        // show_display("diff", &instantaneous_frame_difference, 1, self);

        let mut thresholded = Mat::default();
        opencv::imgproc::threshold(
            &instantaneous_frame_difference,
            &mut thresholded,
            10.0 / 255.0,
            1.0,
            opencv::imgproc::THRESH_BINARY,
        )
        .unwrap();

        // let _white_count = match opencv::core::sum_elems(&thresholded) {
        //     Err(why) => panic!("couldn't sum elems: {}", why),
        //     Ok(v) => v[0] as u32,
        // };

        let mut contours = opencv::types::VectorOfVectorOfPoint::default();
        let mut hierarchy = opencv::core::no_array();
        let mut thresholded_u8 = Mat::default();

        // Necessary for find_contours to work
        thresholded
            .convert_to(&mut thresholded_u8, opencv::core::CV_8U, 255.0, 0.0)
            .unwrap();
        show_display("thresh", &thresholded_u8, 1, self);

        let dilation_size = 1;
        let dilation_element = match opencv::imgproc::get_structuring_element(
            opencv::imgproc::MORPH_ELLIPSE,
            Size::new(dilation_size * 2 + 1, dilation_size * 2 + 1),
            Point::new(dilation_size, dilation_size),
        ) {
            Err(why) => panic!("couldn't get structuring element: {}", why),
            Ok(v) => v,
        };
        let mut thresholded_u8_dilated = Mat::default();
        opencv::imgproc::dilate(
            &thresholded_u8,
            &mut thresholded_u8_dilated,
            &dilation_element,
            Point::new(-1, -1),
            2,
            BORDER_DEFAULT,
            opencv::core::Scalar::new(255.0, 255.0, 255.0, 255.0),
        )
        .unwrap();
        // show_display("thresh and dilate", &thresholded_u8_dilated, 1, self);
        opencv::imgproc::find_contours_with_hierarchy(
            &thresholded_u8_dilated,
            &mut contours,
            &mut hierarchy,
            RETR_EXTERNAL,
            opencv::imgproc::CHAIN_APPROX_SIMPLE,
            Point::new(0, 0),
        )
        .unwrap();

        let mut roi_image = Mat::zeros(self.height as i32, self.width as i32, CV_8U)
            .unwrap()
            .to_mat()
            .unwrap();

        for r in 1..3 {
            for i in 0..contours.len() {
                let contour = contours.get(i).unwrap();
                let area = contour_area(&contour, false).unwrap();

                if area > ((self.width as f32 * self.height as f32).sqrt() * 0.2) as f64
                    && area < ((self.width as f32 * self.height as f32).sqrt() * 10.0) as f64
                {
                    // if area > ((self.width as f32 * self.height as f32) * 0.00001) as f64 {

                    let rect = bounding_rect(&contour).unwrap();

                    rectangle(
                        &mut roi_image,
                        rect,
                        opencv::core::Scalar::new(r as f64, r as f64, r as f64, r as f64),
                        // (self.width as f32 * self.height as f32 * (1.0 / r as f32) * 0.0003) as i32,
                        ((self.width as f32 * self.height as f32).sqrt() * (1.0 / r as f32) * 0.05)
                            as i32,
                        1,
                        0,
                    )
                    .unwrap();
                }
            }
        }
        for i in 0..contours.len() {
            let contour = contours.get(i).unwrap();
            let area = contour_area(&contour, false).unwrap();

            if area > ((self.width as f32 * self.height as f32).sqrt() * 0.2) as f64
                && area < ((self.width as f32 * self.height as f32).sqrt() * 10.0) as f64
            {
                // if area > ((self.width as f32 * self.height as f32) * 0.00001) as f64 {
                let rect = bounding_rect(&contour).unwrap();

                rectangle(
                    &mut self.instantaneous_display_frame,
                    rect,
                    opencv::core::Scalar::new(255.0, 255.0, 255.0, 255.0),
                    2,
                    1,
                    0,
                )
                .unwrap();
                rectangle(
                    &mut roi_image,
                    rect,
                    opencv::core::Scalar::new(6.0, 6.0, 6.0, 6.0),
                    -1,
                    1,
                    0,
                )
                .unwrap();
            }
        }

        let mut roi_normed = Mat::default();

        let scale_factor = self.delta_t_max as f64 / self.ref_time as f64;
        normalize(
            &roi_image,
            &mut roi_normed,
            1.0,
            scale_factor,
            NORM_MINMAX,
            -1,
            &no_array(),
        )
        .unwrap();
        show_display("roi normed", &roi_normed, 1, self);

        let roi_arr = roi_normed.data_bytes().unwrap();
        let chunk_rows: usize = 10;
        let px_per_chunk: usize = chunk_rows * self.width as usize * self.channels as usize;
        self.event_pixels
            .axis_chunks_iter_mut(Axis(0), chunk_rows)
            .into_par_iter()
            .enumerate()
            .for_each(|(chunk_idx, mut chunk)| {
                for (chunk_px_idx, px) in chunk.iter_mut().enumerate() {
                    let px_idx = chunk_px_idx + px_per_chunk * chunk_idx;
                    let factor = &roi_arr[px_idx];
                    px.d_controller.update_roi_factor(*factor);
                }
            });
    }

    pub fn segment_motion(&mut self) {
        // let now = std::time::Instant::now();
        // println!("Seg {}ms", now.elapsed().as_millis());
        let mut motion_seg_overlay = Mat::default();
        add(
            &self.input_frame_8u,
            &self.motion_frame_mat,
            &mut motion_seg_overlay,
            &no_array(),
            0,
        )
        .unwrap();

        let mut thresholded_u8 = self.motion_frame_mat.clone();
        let dilation_size = 1;
        let dilation_element = match opencv::imgproc::get_structuring_element(
            opencv::imgproc::MORPH_ELLIPSE,
            Size::new(dilation_size * 2 + 1, dilation_size * 2 + 1),
            Point::new(dilation_size, dilation_size),
        ) {
            Err(why) => panic!("couldn't get structuring element: {}", why),
            Ok(v) => v,
        };
        let mut thresholded_u8_eroded = Mat::default();
        opencv::imgproc::dilate(
            &thresholded_u8,
            &mut thresholded_u8_eroded,
            &dilation_element,
            Point::new(-1, -1),
            5,
            BORDER_DEFAULT,
            opencv::core::Scalar::new(255.0, 255.0, 255.0, 255.0),
        )
        .unwrap();
        std::mem::swap(&mut thresholded_u8_eroded, &mut thresholded_u8);
        opencv::imgproc::erode(
            &thresholded_u8,
            &mut thresholded_u8_eroded,
            &dilation_element,
            Point::new(-1, -1),
            2,
            BORDER_DEFAULT,
            opencv::core::Scalar::new(255.0, 255.0, 255.0, 255.0),
        )
        .unwrap();

        let mut contours = opencv::types::VectorOfVectorOfPoint::default();
        let mut hierarchy = opencv::core::no_array();

        opencv::imgproc::find_contours_with_hierarchy(
            &thresholded_u8_eroded,
            &mut contours,
            &mut hierarchy,
            RETR_EXTERNAL,
            opencv::imgproc::CHAIN_APPROX_SIMPLE,
            Point::new(0, 0),
        )
        .unwrap();

        for i in 0..contours.len() {
            let contour = contours.get(i).unwrap();
            let area = contour_area(&contour, false).unwrap();
            if area > ((self.width as f32 * self.height as f32).sqrt() * 0.1) as f64
                && area < ((self.width as f32 * self.height as f32).sqrt() * 20.0) as f64
            {
                todo!();
                let rect = bounding_rect(&contour).unwrap();
                rectangle(
                    &mut self.instantaneous_display_frame,
                    rect,
                    opencv::core::Scalar::new(255.0, 255.0, 255.0, 255.0),
                    2,
                    1,
                    0,
                )
                .unwrap();
            }
        }

        //// Disabled for speed at the moment
        show_display("Motion seg overlay", &motion_seg_overlay, 1, self);
        // show_display("Motion seg raw", &motion_seg_raw, 1, self);
        show_display("Motion seg eroded", &thresholded_u8_eroded, 1, self);
        // let mut rate_mat_norm = Mat::default();
        // normalize(&self.rate_frame_mat, &mut rate_mat_norm, 122.0, 255.0, NORM_MINMAX, -1, &no_array()).unwrap();
        // show_display("rate code", &rate_mat_norm, 1, self);
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
    fn consume(&mut self, view_interval: u32) -> Result<Vec<Vec<Event>>, &'static str>;

    fn get_video_mut(&mut self) -> &mut Video;

    fn get_video(&self) -> &Video;
}
