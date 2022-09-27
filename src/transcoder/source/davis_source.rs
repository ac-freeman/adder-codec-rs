use crate::transcoder::d_controller::DecimationMode;
use crate::transcoder::source::video::{Source, SourceError, Video};
use crate::SourceCamera::DavisU8;
use crate::{DeltaT, Event};
use davis_edi_rs::util::reconstructor::Reconstructor;
use davis_edi_rs::*;
use opencv::core::Mat;
use opencv::{imgproc, prelude::*, videoio, Result};
use std::sync::mpsc::{Receiver, Sender};

/// Attributes of a framed video -> ADΔER transcode
pub struct DavisSource {
    reconstructor: Reconstructor,
    pub(crate) input_frame_scaled: Box<Mat>,
    c_thresh_pos: u8,
    c_thresh_neg: u8,

    pub(crate) video: Video,
}

impl DavisSource {
    /// Initialize the framed source and read first frame of source, in order to get `height`
    /// and `width` and initialize [`Video`]
    fn new(
        mut reconstructor: Reconstructor,
        output_events_filename: Option<String>,
        tps: DeltaT,
        delta_t_max: DeltaT,
        show_display_b: bool,
    ) -> Result<DavisSource> {
        let video = Video::new(
            reconstructor.width as u16,
            reconstructor.height as u16,
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
        );
        let davis_source = DavisSource {
            reconstructor,
            input_frame_scaled: Box::new(Default::default()),
            c_thresh_pos: 15, // TODO
            c_thresh_neg: 15, // TODO
            video,
        };
        Ok(davis_source)
    }
}

impl Source for DavisSource {
    fn consume(&mut self, view_interval: u32) -> std::result::Result<Vec<Vec<Event>>, SourceError> {
        todo!()
    }

    fn get_video_mut(&mut self) -> &mut Video {
        &mut self.video
    }

    fn get_video(&self) -> &Video {
        &self.video
    }
}
