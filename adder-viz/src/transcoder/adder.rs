use std::error::Error;
use std::ffi::{OsStr, OsString};

#[cfg(feature = "open-cv")]
use adder_codec_rs::transcoder::source::davis::Davis;
use adder_codec_rs::transcoder::source::framed::Framed;
use eframe::epaint::ColorImage;
use egui::Color32;
use std::fmt;
use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

#[cfg(feature = "open-cv")]
use adder_codec_rs::transcoder::source::davis::TranscoderMode;

#[cfg(feature = "open-cv")]
use adder_codec_rs::davis_edi_rs::util::reconstructor::Reconstructor;

use crate::transcoder::adder::AdderTranscoderError::{
    InvalidFileType, NoFileSelected, Uninitialized,
};
use crate::transcoder::ui::{TranscoderInfoMsg, TranscoderState, TranscoderStateMsg};
use crate::transcoder::{EventRateMsg, InfoUiState};
use crate::utils::prep_epaint_image;
use crate::Images;
use adder_codec_rs::adder_codec_core::codec::rate_controller::DEFAULT_CRF_QUALITY;
use adder_codec_rs::adder_codec_core::SourceCamera::{DavisU8, Dvs, FramedU8};
use adder_codec_rs::adder_codec_core::{Event, PlaneError};
use adder_codec_rs::davis_edi_rs::util::reconstructor::ReconstructorError;
use adder_codec_rs::transcoder::source::prophesee::Prophesee;
use adder_codec_rs::transcoder::source::video::SourceError::VideoError;
use adder_codec_rs::transcoder::source::video::{Source, SourceError, VideoBuilder};
use adder_codec_rs::transcoder::source::AdderSource;
use adder_codec_rs::utils::cv::{calculate_quality_metrics, QualityMetrics};
#[cfg(feature = "open-cv")]
use opencv::Result;
use thiserror::Error;
use tokio::sync::mpsc;
use tokio::sync::mpsc::error::{TryRecvError, TrySendError};
use tokio::sync::mpsc::Receiver;
use video_rs_adder_dep::Frame;

pub struct AdderTranscoder {
    pool: tokio::runtime::Runtime,
    transcoder_state: TranscoderState,
    source: Option<AdderSource<BufWriter<File>>>,
    rx: Receiver<TranscoderStateMsg>,
    msg_tx: mpsc::Sender<TranscoderInfoMsg>,
    pub(crate) input_image_handle: egui::TextureHandle,
    pub(crate) adder_image_handle: egui::TextureHandle,
    total_events: u64,
    last_consume_time: std::time::Instant,
}

#[derive(Error, Debug)]
pub enum AdderTranscoderError {
    /// Input file error
    #[error("Invalid file type")]
    InvalidFileType,

    /// No file selected
    #[error("No file selected")]
    NoFileSelected,

    /// Source error
    #[error("Source error")]
    SourceError(#[from] SourceError),

    /// IO error
    #[error("IO error")]
    IoError(#[from] std::io::Error),

    /// Other error
    #[error("Other error")]
    OtherError(#[from] Box<dyn Error>),

    #[error(transparent)]
    ReconstructorError(#[from] ReconstructorError),

    /// Uninitialized error
    #[error("Uninitialized")]
    Uninitialized,
}

impl AdderTranscoder {
    pub(crate) fn new(
        rx: Receiver<TranscoderStateMsg>,
        msg_tx: mpsc::Sender<TranscoderInfoMsg>,
        input_image_handle: egui::TextureHandle,
        adder_image_handle: egui::TextureHandle,
    ) -> Self {
        let threaded_rt = tokio::runtime::Runtime::new().unwrap();

        AdderTranscoder {
            pool: threaded_rt,
            transcoder_state: Default::default(),
            source: None,
            rx,
            msg_tx,
            input_image_handle,
            adder_image_handle,
            total_events: 0,
            last_consume_time: std::time::Instant::now(),
        }
    }

    /// The unbounded loop. Continually processes messages or consumes the source
    pub(crate) async fn run(&mut self) {
        loop {
            match self.rx.try_recv() {
                Ok(msg) => match msg {
                    TranscoderStateMsg::Terminate => {
                        eprintln!("Resetting video");

                        if let Some(source) = &mut self.source {
                            // Get the current source and close the writer
                            source.get_video_mut().end_write_stream().unwrap();
                        }
                        self.source = None;
                        self.total_events = 0;

                        self.transcoder_state.core_params.input_path_buf_0 = None;
                        self.transcoder_state.core_params.output_path = None;

                        // Clear the images
                        self.adder_image_handle
                            .set(ColorImage::default(), Default::default());
                        self.input_image_handle
                            .set(ColorImage::default(), Default::default());
                    }

                    TranscoderStateMsg::Set { transcoder_state } => {
                        eprintln!("Received transcoder state");
                        let result = self.state_update(transcoder_state, false).await;
                        self.handle_error(result);
                    }
                },
                Err(_) => {
                    // Received no data, so consume the transcoder source if it exists
                    if self.source.is_some() {
                        let result = self.consume();
                        self.handle_error(result);
                    }
                }
            }
        }
    }

    async fn handle_error(&mut self, result: Result<(), AdderTranscoderError>) {
        match result {
            Ok(()) => {}
            Err(e) => {
                match e {
                    InvalidFileType => {}
                    NoFileSelected => {}
                    AdderTranscoderError::SourceError(VideoError(
                        video_rs_adder_dep::Error::ReadExhausted,
                    )) => {
                        let mut state = self.transcoder_state.clone();
                        self.source
                            .as_mut()
                            .unwrap()
                            .get_video_mut()
                            .state
                            .in_interval_count = 0;
                        state.core_params.output_path = None;
                        self.state_update(state, true)
                            .await
                            .expect("Error creating new transcoder");
                        return;
                    }
                    AdderTranscoderError::SourceError(_) => {}
                    AdderTranscoderError::IoError(_) => {}
                    AdderTranscoderError::OtherError(_) => {}
                    Uninitialized => {}
                    _ => {}
                }

                match self
                    .msg_tx
                    .try_send(TranscoderInfoMsg::Error(e.to_string()))
                {
                    Err(TrySendError::Full(..)) => {
                        dbg!(e);
                        eprintln!("Msg channel full");
                    }
                    _ => {}
                };
            }
        }
    }

    fn consume(&mut self) -> Result<(), AdderTranscoderError> {
        {
            let source = self.source.as_mut().ok_or(Uninitialized)?;
            let result: Vec<Vec<Event>> = source.consume()?;
            let mut msg = EventRateMsg::default();

            for events_vec in result {
                self.total_events += events_vec.len() as u64;
                msg.events_per_sec += events_vec.len() as f64;
            }
            msg.events_ppc_total =
                self.total_events as f64 / (source.get_video_ref().state.plane.volume() as f64);
            let source_fps = source.get_video_ref().get_tps() as f64
                / source.get_video_ref().get_ref_time() as f64;
            msg.events_per_sec *= source_fps;
            msg.events_ppc_per_sec =
                msg.events_per_sec / (source.get_video_ref().state.plane.volume() as f64);
            msg.total_events = self.total_events;

            msg.transcoded_fps = 1.0
                / Instant::now()
                    .duration_since(self.last_consume_time)
                    .as_secs_f64();
            msg.num_pixels = source.get_video_ref().state.plane.volume() as u64;
            msg.running_input_bitrate = source.get_running_input_bitrate();

            // Send the message
            match self.msg_tx.try_send(TranscoderInfoMsg::EventRateMsg(msg)) {
                Ok(_) => {}
                Err(TrySendError::Full(..)) => {
                    // eprintln!("Event rate channel full");
                }
                Err(e) => {
                    // return Err(Box::new(e)); // TODO
                }
            };
        }
        self.show_input_frame();

        // Display frame
        self.show_display_frame();

        self.quality_metrics();

        self.last_consume_time = std::time::Instant::now();

        Ok(())
    }

    fn show_input_frame(&mut self) {
        if let Some(AdderSource::Framed(source)) = &mut self.source {
            // Check if source is the Framed enum variant

            let image_mat = source.get_input().unwrap();
            let color = image_mat.shape()[2] == 3;
            let width = image_mat.shape()[1];
            let height = image_mat.shape()[0];

            let image = prep_epaint_image(image_mat, color, width, height).unwrap();

            self.input_image_handle.set(image, Default::default());
        }
    }

    fn show_display_frame(&mut self) {
        let image_mat = &self
            .source
            .as_ref()
            .unwrap()
            .get_video_ref()
            .display_frame_features;
        let color = image_mat.shape()[2] == 3;
        let width = image_mat.shape()[1];
        let height = image_mat.shape()[0];

        let image = prep_epaint_image(image_mat, color, width, height).unwrap();

        self.adder_image_handle.set(image, Default::default());
    }

    async fn state_update(
        &mut self,
        transcoder_state: TranscoderState,
        force_new: bool,
    ) -> Result<(), AdderTranscoderError> {
        if force_new || transcoder_state.core_params != self.transcoder_state.core_params {
            eprintln!("Create new transcoder");
            let res = self.core_state_update(transcoder_state).await;
            if res.is_ok() {
                // Send a message with the plane size of the video
                let plane = self
                    .source
                    .as_ref()
                    .unwrap()
                    .get_video_ref()
                    .state
                    .plane
                    .clone();
                match self
                    .msg_tx
                    .try_send(TranscoderInfoMsg::Plane((plane, force_new)))
                {
                    Ok(_) => {}
                    Err(TrySendError::Full(..)) => {
                        eprintln!("Metrics channel full");
                    }
                    Err(e) => {
                        panic!("todo");
                    }
                };
            }
            return res;
        } else if transcoder_state.adaptive_params != self.transcoder_state.adaptive_params {
            eprintln!("Modify existing transcoder");
            self.update_params(transcoder_state);
            return self.adaptive_state_update();
        } else {
            eprintln!("No change in transcoder state");
        }
        self.update_params(transcoder_state);

        Ok(())
    }

    fn quality_metrics(&self) -> Result<(), Box<dyn Error>> {
        if !(self.transcoder_state.info_params.metric_mse
            || self.transcoder_state.info_params.metric_psnr
            || self.transcoder_state.info_params.metric_ssim)
        {
            return Ok(());
        }
        if let Some(AdderSource::Framed(source)) = &self.source {
            let input = source.get_input().unwrap();
            let output = &source.get_video_ref().state.running_intensities;

            #[rustfmt::skip]
                let metrics = calculate_quality_metrics(
                input,
                output,
                QualityMetrics {
                    mse: if self.transcoder_state.info_params.metric_mse { Some(0.0) } else { None },
                    psnr: if self.transcoder_state.info_params.metric_psnr { Some(0.0) } else { None },
                    ssim: if self.transcoder_state.info_params.metric_ssim { Some(0.0) } else { None },
                })?;

            match self
                .msg_tx
                .try_send(TranscoderInfoMsg::QualityMetrics(metrics))
            {
                Ok(_) => {}
                Err(TrySendError::Full(..)) => {
                    eprintln!("Metrics channel full");
                }
                Err(e) => {
                    return Err(Box::new(e));
                }
            };
        }

        Ok(())
    }

    /// Called both when creating a new transcoder source and when an adaptive parameter has
    /// changed. Sets the adaptive parameters for the source. Sets all the parameters (instead of
    /// only the changed ones) because it's much easier to read and it's still fast.
    fn adaptive_state_update(&mut self) -> Result<(), AdderTranscoderError> {
        let source = self.source.as_mut().ok_or(Uninitialized)?;

        let params = &self.transcoder_state.adaptive_params;
        source.get_video_mut().instantaneous_view_mode = params.view_mode_radio_state;
        source.get_video_mut().update_detect_features(
            params.detect_features,
            params.show_features,
            params.feature_rate_adjustment,
            params.feature_cluster,
        );
        let quality_parameters = params.encoder_options.crf.get_parameters();
        source.get_video_mut().update_quality_manual(
            quality_parameters.c_thresh_baseline,
            quality_parameters.c_thresh_max,
            self.transcoder_state.core_params.delta_t_max_mult,
            quality_parameters.c_increase_velocity,
            quality_parameters.feature_c_radius as f32,
        );

        Ok(())
    }

    async fn core_state_update(
        &mut self,
        transcoder_state: TranscoderState,
    ) -> Result<(), AdderTranscoderError> {
        self.total_events = 0;
        match &transcoder_state.core_params.input_path_buf_0 {
            None => Err(NoFileSelected),
            Some(input_path_buf) => match input_path_buf.extension() {
                None => Err(InvalidFileType),
                Some(ext) => match ext.to_ascii_lowercase().to_str().unwrap() {
                    "mp4" | "mkv" | "avi" | "mov" => {
                        // Framed video
                        self.create_framed(transcoder_state)
                    }
                    "aedat4" | "sock" => {
                        // Davis video
                        let ext = ext.to_os_string();
                        #[cfg(feature = "open-cv")]
                        self.create_davis(transcoder_state, ext).await
                    }
                    // "dat" => {
                    //     // Prophesee video
                    // }
                    _ => Err(InvalidFileType),
                },
            },
        }
    }

    fn update_params(&mut self, transcoder_state: TranscoderState) {
        self.transcoder_state = transcoder_state;
    }

    fn create_framed(
        &mut self,
        transcoder_state: TranscoderState,
    ) -> Result<(), AdderTranscoderError> {
        let mut current_frame = 0;
        // If we already have an adder_transcoder, get the current frame
        if let Some(AdderSource::Framed(source)) = &mut self.source {
            if transcoder_state.core_params.input_path_buf_0
                == self.transcoder_state.core_params.input_path_buf_0
                && transcoder_state.core_params.output_path.is_none()
                && self.transcoder_state.core_params.output_path.is_none()
            {
                current_frame =
                    source.get_video_ref().state.in_interval_count + source.frame_idx_start;
            }
            source.get_video_mut().end_write_stream()?;
        }

        self.update_params(transcoder_state);

        let core_params = &self.transcoder_state.core_params;
        let adaptive_params = &self.transcoder_state.adaptive_params;

        let mut framed = Framed::new(
            core_params.input_path_buf_0.clone().unwrap(),
            core_params.color,
            core_params.scale,
        )?
        .crf(
            adaptive_params
                .encoder_options
                .crf
                .get_quality()
                .unwrap_or(DEFAULT_CRF_QUALITY),
        )
        .frame_start(current_frame)?
        .chunk_rows(1)
        .auto_time_parameters(
            core_params.delta_t_ref as u32,
            core_params.delta_t_max_mult * core_params.delta_t_ref as u32,
            Some(core_params.time_mode),
        )?;

        // TODO: Change the builder to take in a pathbuf directly, not a string,
        // and to handle the error checking in the associated function
        match &core_params.output_path {
            None => {}
            Some(output_path) => {
                let out_path = output_path.to_str().unwrap();
                let writer = BufWriter::new(File::create(out_path)?);

                framed = *framed.write_out(
                    FramedU8,
                    core_params.time_mode,
                    core_params.integration_mode_radio_state,
                    Some(core_params.adu_interval as usize),
                    core_params.encoder_type,
                    adaptive_params.encoder_options,
                    writer,
                )?;
            }
        };

        self.source = Some(AdderSource::Framed(framed));

        self.adaptive_state_update()?;
        self.last_consume_time = std::time::Instant::now();

        eprintln!("Framed source created!");
        Ok(())
    }

    async fn create_davis(
        &mut self,
        transcoder_state: TranscoderState,
        ext: OsString,
    ) -> Result<(), AdderTranscoderError> {
        self.update_params(transcoder_state);

        let core_params = &self.transcoder_state.core_params;
        let adaptive_params = &self.transcoder_state.adaptive_params;

        let events_only = match &core_params.davis_mode_radio_state {
            TranscoderMode::Framed => false,
            TranscoderMode::RawDavis => false,
            TranscoderMode::RawDvs => true,
        };
        let deblur_only = match &core_params.davis_mode_radio_state {
            TranscoderMode::Framed => false,
            TranscoderMode::RawDavis => true,
            TranscoderMode::RawDvs => true,
        };

        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(8) // TODO: get from a slider
            .enable_time()
            .build()?;
        let dir = core_params
            .input_path_buf_0
            .clone()
            .unwrap()
            .parent()
            .expect("File must be in some directory")
            .to_str()
            .expect("Bad path")
            .to_string();
        let filename_0 = core_params
            .input_path_buf_0
            .clone()
            .unwrap()
            .file_name()
            .expect("File must exist")
            .to_str()
            .expect("Bad filename")
            .to_string();

        let mut mode = "file";
        let mut simulate_latency = true;
        if ext == "sock" {
            mode = "socket";
            simulate_latency = false;
        }

        // let filename_1 = _input_path_buf_1.as_ref().map(|_input_path_buf_1| {
        //     _input_path_buf_1
        //         .file_name()
        //         .expect("File must exist")
        //         .to_str()
        //         .expect("Bad filename")
        //         .to_string()
        // });
        let filename_1 = None;

        let reconstructor: Reconstructor = Reconstructor::new(
            dir + "/",
            filename_0,
            filename_1.unwrap_or("".to_string()),
            mode.to_string(), // TODO
            0.15,
            adaptive_params.optimize_c,
            adaptive_params.optimize_c_frequency,
            false,
            false,
            false,
            core_params.davis_output_fps,
            deblur_only,
            events_only,
            1000.0, // Target latency (not used)
            simulate_latency,
        )
        .await?;

        let output_string = core_params
            .output_path
            .clone()
            .map(|output_path| output_path.to_str().expect("Bad path").to_string());

        let mut davis_source: Davis<BufWriter<File>> =
            Davis::new(reconstructor, rt, core_params.davis_mode_radio_state)?
                .optimize_adder_controller(false) // TODO
                .mode(core_params.davis_mode_radio_state)
                .crf(
                    adaptive_params
                        .encoder_options
                        .crf
                        .get_quality()
                        .unwrap_or(DEFAULT_CRF_QUALITY),
                )
                .time_parameters(
                    20000000_u32,
                    (1_000_000.0 / core_params.davis_output_fps)
                        as adder_codec_rs::adder_codec_core::DeltaT,
                    20000000_u32,
                    Some(core_params.time_mode),
                )?;

        // Override time parameters if we're in framed mode
        if core_params.davis_mode_radio_state == TranscoderMode::Framed {
            davis_source = davis_source.time_parameters(
                (255.0 * core_params.davis_output_fps) as u32,
                255,
                255 * core_params.delta_t_max_mult,
                Some(core_params.time_mode),
            )?;
        }

        if let Some(output_string) = output_string {
            let writer = BufWriter::new(File::create(output_string)?);
            davis_source = *davis_source.write_out(
                DavisU8,
                core_params.time_mode,
                core_params.integration_mode_radio_state,
                Some(core_params.delta_t_max_mult as usize),
                core_params.encoder_type,
                adaptive_params.encoder_options,
                writer,
            )?;
        }

        self.source = Some(AdderSource::Davis(davis_source));

        self.adaptive_state_update()?;
        self.last_consume_time = std::time::Instant::now();

        eprintln!("Davis source created!");
        Ok(())
    }
}
