use crate::transcoder::d_controller::DecimationMode;
use crate::transcoder::event_pixel_tree::Mode::Continuous;
use crate::transcoder::source::video::SourceError::BufferEmpty;
use crate::transcoder::source::video::{Source, SourceError, Video};
use crate::SourceCamera::DavisU8;
use crate::{DeltaT, Event};
use aedat::events_generated::Event as DvsEvent;
use davis_edi_rs::util::reconstructor::{IterVal, Reconstructor};
use std::marker::PhantomData;

use opencv::core::{Mat, CV_8U};
use opencv::{prelude::*, Result};

use rayon::{current_num_threads, ThreadPool};
use std::cmp::max;

use tokio::runtime::Runtime;

// https://stackoverflow.com/questions/51344951/how-do-you-unwrap-a-result-on-ok-or-return-from-the-function-on-err
macro_rules! unwrap_or_return {
    ( $e:expr ) => {
        match $e {
            Some(x) => x,
            None => return,
        }
    };
}

pub struct Framed {}
pub struct Raw {}

pub enum DavisTranscoderMode {
    Framed,
    Raw,
}

/// Attributes of a framed video -> ADΔER transcode
pub struct DavisSource {
    reconstructor: Reconstructor,
    pub(crate) input_frame_scaled: Mat,
    pub(crate) video: Video,
    image_8u: Mat,
    thread_pool_edi: ThreadPool,
    thread_pool_integration: ThreadPool,
    dvs_events: Option<Vec<DvsEvent>>,
    end_of_frame_timestamp: Option<i64>,
    pub rt: Runtime,
    mode: DavisTranscoderMode, // phantom: PhantomData<T>,
}

impl DavisSource {
    /// Initialize the framed source and read first frame of source, in order to get `height`
    /// and `width` and initialize [`Video`]
    pub fn new(
        reconstructor: Reconstructor,
        output_events_filename: Option<String>,
        tps: DeltaT,
        delta_t_max: DeltaT,
        show_display_b: bool,
        adder_c_thresh_pos: u8,
        adder_c_thresh_neg: u8,
        rt: Runtime,
        mode: DavisTranscoderMode,
    ) -> Result<DavisSource> {
        let video = Video::new(
            reconstructor.width as u16,
            reconstructor.height as u16,
            64,
            output_events_filename,
            1,
            tps,
            (tps as f64 / reconstructor.output_fps) as u32,
            delta_t_max,
            DecimationMode::Manual,
            true, // TODO
            true, // TODO
            show_display_b,
            DavisU8,
            adder_c_thresh_pos,
            adder_c_thresh_neg,
        );
        let thread_pool_edi = rayon::ThreadPoolBuilder::new()
            .num_threads(max(current_num_threads() - 4, 1))
            .build()
            .unwrap();
        let thread_pool_integration = rayon::ThreadPoolBuilder::new()
            .num_threads(max(4, 1))
            .build()
            .unwrap();

        let davis_source = DavisSource {
            reconstructor,
            input_frame_scaled: Mat::default(),
            video,
            image_8u: Mat::default(),
            thread_pool_edi,
            thread_pool_integration,
            dvs_events: None,
            end_of_frame_timestamp: None,
            rt,
            mode,
        };
        Ok(davis_source)
    }

    // TODO: need to return the events for simultaneously reframing?
    pub fn integrate_dvs_events(&mut self) {
        // Using a macro so that CLion still pretty prints correctly
        let dvs_events = unwrap_or_return!(self.dvs_events.as_ref());
        let end_of_frame_timestamp = unwrap_or_return!(self.end_of_frame_timestamp.as_ref());
        for event in dvs_events.iter() {
            if event.t() > *end_of_frame_timestamp {
                println!(" ");
            }
        }
    }
}

impl Source for DavisSource {
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
            DavisTranscoderMode::Framed => false,
            DavisTranscoderMode::Raw => true,
        };
        let mat_opt = self.rt.block_on(get_next_image(
            &mut self.reconstructor,
            &self.thread_pool_edi,
            with_events,
        ));
        match mat_opt {
            None => {
                return Err(SourceError::NoData);
            }
            Some((mat, Some((events, timestamp)))) => {
                self.input_frame_scaled = mat;
                self.dvs_events = Some(events);
                self.end_of_frame_timestamp = Some(timestamp);
            }
            Some((mat, None)) => {
                self.input_frame_scaled = mat;
            }
        }

        if self.input_frame_scaled.empty() {
            eprintln!("End of video");
            return Err(BufferEmpty);
        }

        self.input_frame_scaled
            .convert_to(&mut self.image_8u, CV_8U, 255.0, 0.0)
            .unwrap();

        // While `input_frame_scaled` may not be continuous (which would cause problems with
        // iterating over the pixels), cloning it ensures that it is made continuous.
        // https://stackoverflow.com/questions/33665241/is-opencv-matrix-data-guaranteed-to-be-continuous
        let tmp = self.image_8u.clone();
        thread_pool.install(|| {
            self.video
                .integrate_matrix(tmp, self.video.ref_time as f32, Continuous, view_interval)
        })
    }

    fn get_video_mut(&mut self) -> &mut Video {
        &mut self.video
    }

    fn get_video(&self) -> &Video {
        &self.video
    }
}

async fn get_next_image(
    reconstructor: &mut Reconstructor,
    thread_pool: &ThreadPool,
    with_events: bool,
) -> Option<IterVal> {
    thread_pool
        .install(|| async {
            match reconstructor.next(with_events).await {
                None => {
                    println!("\nFinished!");
                    None
                }
                Some(res) => match res {
                    Ok(a) => Some(a),
                    Err(_) => {
                        panic!("No image")
                    }
                },
            }
        })
        .await
}
