use std::error::Error;
use std::ffi::OsStr;

#[cfg(feature = "open-cv")]
use adder_codec_rs::transcoder::source::davis::Davis;
use adder_codec_rs::transcoder::source::framed::Framed;
use std::fmt;
use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};

#[cfg(feature = "open-cv")]
use adder_codec_rs::transcoder::source::davis::TranscoderMode;

#[cfg(feature = "open-cv")]
use adder_codec_rs::davis_edi_rs::util::reconstructor::Reconstructor;

use crate::transcoder::adder::AdderTranscoderError::InvalidFileType;
use crate::transcoder::ui::{AdaptiveParams, TranscoderState, TranscoderStateMsg};
use crate::Images;
use adder_codec_rs::adder_codec_core::codec::rate_controller::DEFAULT_CRF_QUALITY;
use adder_codec_rs::adder_codec_core::PlaneError;
use adder_codec_rs::adder_codec_core::SourceCamera::{DavisU8, Dvs, FramedU8};
use adder_codec_rs::transcoder::source::prophesee::Prophesee;
use adder_codec_rs::transcoder::source::video::{Source, SourceError, VideoBuilder};
#[cfg(feature = "open-cv")]
use opencv::Result;
use thiserror::Error;
use tokio::sync::mpsc::error::TryRecvError;
use tokio::sync::mpsc::Receiver;

pub struct AdderTranscoder {
    transcoder_state: TranscoderState,
    pub(crate) framed_source: Option<Framed<BufWriter<File>>>,
    #[cfg(feature = "open-cv")]
    pub(crate) davis_source: Option<Davis<BufWriter<File>>>,
    pub(crate) prophesee_source: Option<Prophesee<BufWriter<File>>>,
    rx: Receiver<TranscoderStateMsg>,
    pub(crate) images: std::sync::Arc<Images>,
}

#[derive(Error, Debug)]
pub enum AdderTranscoderError {
    /// Input file error
    #[error("Invalid file type")]
    InvalidFileType,

    /// Plane error
    #[error("Source error")]
    SourceError(#[from] SourceError),

    /// IO error
    #[error("IO error")]
    IoError(#[from] std::io::Error),
}

impl AdderTranscoder {
    pub(crate) fn new(rx: Receiver<TranscoderStateMsg>, images: std::sync::Arc<Images>) -> Self {
        AdderTranscoder {
            transcoder_state: Default::default(),
            framed_source: None,
            #[cfg(feature = "open-cv")]
            davis_source: None,
            prophesee_source: None,
            rx,
            images,
        }
    }

    pub(crate) fn run(&mut self) {
        loop {
            // eprintln!("Waiting to receive data");
            // Sleep for 1 second
            // std::thread::sleep(std::time::Duration::from_secs(1));
            match self.rx.try_recv() {
                Ok(msg) => match msg {
                    TranscoderStateMsg::Terminate => {
                        break;
                    }
                    TranscoderStateMsg::Set { transcoder_state } => {
                        eprintln!("Received transcoder state");
                        let result = self.state_update(transcoder_state);
                        self.handle_error(result);
                    }
                },
                Err(_) => {
                    // eprintln!("Received no data");
                }
            }
        }
    }

    fn handle_error(&mut self, result: Result<(), AdderTranscoderError>) {
        match result {
            Ok(()) => {}
            Err(e) => {
                eprintln!("TODO: handle Error: {}", e);
            }
        }
    }

    fn state_update(
        &mut self,
        transcoder_state: TranscoderState,
    ) -> Result<(), AdderTranscoderError> {
        if transcoder_state.core_params != self.transcoder_state.core_params {
            eprintln!("Create new transcoder");
            return self.core_state_update(transcoder_state);
        } else if transcoder_state.adaptive_params != self.transcoder_state.adaptive_params {
            eprintln!("Modify existing transcoder");
            return Ok(());
        }
        Ok(())
    }

    fn core_state_update(
        &mut self,
        transcoder_state: TranscoderState,
    ) -> Result<(), AdderTranscoderError> {
        match &transcoder_state.core_params.input_path_buf_0 {
            None => Err(InvalidFileType),
            Some(input_path_buf) => match input_path_buf.extension() {
                None => Err(InvalidFileType),
                Some(ext) => match ext.to_ascii_lowercase().to_str().unwrap() {
                    "mp4" | "mkv" | "avi" | "mov" => {
                        // Framed video
                        self.create_framed(transcoder_state)
                    }
                    // "aedat4" | "sock" => {
                    //     // Davis video
                    // }
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
        // If we already have an adder_transcoder, get the current frame
        let current_frame = match &self.framed_source {
            None => 0,
            Some(framed) => {
                if transcoder_state.core_params.input_path_buf_0
                    == self.transcoder_state.core_params.input_path_buf_0
                {
                    framed.get_video_ref().state.in_interval_count + framed.frame_idx_start
                } else {
                    0
                }
            }
        };

        self.update_params(transcoder_state);

        let core_params = &self.transcoder_state.core_params;
        let adaptive_params = &self.transcoder_state.adaptive_params;

        let mut framed: Framed<BufWriter<File>> = Framed::new(
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
                    adaptive_params.integration_mode_radio_state,
                    Some(core_params.delta_t_max_mult as usize),
                    core_params.encoder_type,
                    adaptive_params.encoder_options,
                    writer,
                )?;
            }
        };

        self.framed_source = Some(framed);
        #[cfg(feature = "open-cv")]
        {
            self.davis_source = None;
        }
        self.prophesee_source = None;

        eprintln!("Framed source created!");
        Ok(())
    }

    //     match input_path_buf.extension() {
    //         None => Err(Box::new(AdderTranscoderError("Invalid file type".into()))),
    //         Some(ext) => {
    //             match ext.to_ascii_lowercase().to_str() {
    //                 None => Err(Box::new(AdderTranscoderError("Invalid file type".into()))),
    //                 Some("mp4") => {
    //                     let mut framed: Framed<BufWriter<File>> = Framed::new(
    //                         match input_path_buf.to_str() {
    //                             None => {
    //                                 return Err(Box::new(AdderTranscoderError(
    //                                     "Couldn't get input path string".into(),
    //                                 )))
    //                             }
    //                             Some(path) => path.to_string(),
    //                         },
    //                         ui_state.color,
    //                         ui_state.scale,
    //                     )?
    //                     .crf(
    //                         ui_state
    //                             .encoder_options
    //                             .crf
    //                             .get_quality()
    //                             .unwrap_or(DEFAULT_CRF_QUALITY),
    //                     )
    //                     .frame_start(current_frame)?
    //                     .chunk_rows(1)
    //                     .auto_time_parameters(
    //                         ui_state.delta_t_ref as u32,
    //                         ui_state.delta_t_max_mult * ui_state.delta_t_ref as u32,
    //                         Some(ui_state.time_mode),
    //                     )?;
    //
    //                     // TODO: Change the builder to take in a pathbuf directly, not a string,
    //                     // and to handle the error checking in the associated function
    //                     match output_path_opt {
    //                         None => {}
    //                         Some(output_path) => {
    //                             let out_path = output_path.to_str().unwrap();
    //                             let writer = BufWriter::new(File::create(out_path)?);
    //
    //                             framed = *framed.write_out(
    //                                 FramedU8,
    //                                 ui_state.time_mode,
    //                                 ui_state.integration_mode_radio_state,
    //                                 Some(ui_state.delta_t_max_mult as usize),
    //                                 ui_state.encoder_type,
    //                                 ui_state.encoder_options,
    //                                 writer,
    //                             )?;
    //                         }
    //                     };
    //
    //                     // ui_state.delta_t_ref_max = 255.0; // TODO: reimplement this
    //                     Ok(AdderTranscoder {
    //                         framed_source: Some(framed),
    //                         #[cfg(feature = "open-cv")]
    //                         davis_source: None,
    //                         prophesee_source: None,
    //                         // live_image: Default::default(),
    //                     })
    //                     // }
    //                     // Err(_e) => {
    //                     //     Err(Box::new(AdderTranscoderError("Invalid file type".into())))
    //                     // }
    //                 }
    //
    //                 #[cfg(feature = "open-cv")]
    //                 Some(ext) if ext == "aedat4" || ext == "sock" => {
    //                     let events_only = match &ui_state.davis_mode_radio_state {
    //                         TranscoderMode::Framed => false,
    //                         TranscoderMode::RawDavis => false,
    //                         TranscoderMode::RawDvs => true,
    //                     };
    //                     let deblur_only = match &ui_state.davis_mode_radio_state {
    //                         TranscoderMode::Framed => false,
    //                         TranscoderMode::RawDavis => true,
    //                         TranscoderMode::RawDvs => true,
    //                     };
    //
    //                     let rt = tokio::runtime::Builder::new_multi_thread()
    //                         .worker_threads(ui_state.thread_count)
    //                         .enable_time()
    //                         .build()?;
    //                     let dir = input_path_buf
    //                         .parent()
    //                         .expect("File must be in some directory")
    //                         .to_str()
    //                         .expect("Bad path")
    //                         .to_string();
    //                     let filename_0 = input_path_buf
    //                         .file_name()
    //                         .expect("File must exist")
    //                         .to_str()
    //                         .expect("Bad filename")
    //                         .to_string();
    //
    //                     let mut mode = "file";
    //                     let mut simulate_latency = true;
    //                     if ext == "sock" {
    //                         mode = "socket";
    //                         simulate_latency = false;
    //                     }
    //
    //                     let filename_1 = _input_path_buf_1.as_ref().map(|_input_path_buf_1| {
    //                         _input_path_buf_1
    //                             .file_name()
    //                             .expect("File must exist")
    //                             .to_str()
    //                             .expect("Bad filename")
    //                             .to_string()
    //                     });
    //
    //                     let reconstructor = rt.block_on(Reconstructor::new(
    //                         dir + "/",
    //                         filename_0,
    //                         filename_1.unwrap_or("".to_string()),
    //                         mode.to_string(), // TODO
    //                         0.15,
    //                         ui_state.optimize_c,
    //                         ui_state.optimize_c_frequency,
    //                         false,
    //                         false,
    //                         false,
    //                         ui_state.davis_output_fps,
    //                         deblur_only,
    //                         events_only,
    //                         1000.0, // Target latency (not used)
    //                         simulate_latency,
    //                     ))?;
    //
    //                     let output_string = output_path_opt
    //                         .map(|output_path| output_path.to_str().expect("Bad path").to_string());
    //
    //                     let mut davis_source: Davis<BufWriter<File>> =
    //                         Davis::new(reconstructor, rt, ui_state.davis_mode_radio_state)?
    //                             .optimize_adder_controller(false) // TODO
    //                             .mode(ui_state.davis_mode_radio_state)
    //                             .crf(
    //                                 ui_state
    //                                     .encoder_options
    //                                     .crf
    //                                     .get_quality()
    //                                     .unwrap_or(DEFAULT_CRF_QUALITY),
    //                             )
    //                             .time_parameters(
    //                                 1000000_u32,
    //                                 (1_000_000.0 / ui_state.davis_output_fps)
    //                                     as adder_codec_rs::adder_codec_core::DeltaT,
    //                                 ((1_000_000.0 / ui_state.davis_output_fps)
    //                                     * ui_state.delta_t_max_mult as f64)
    //                                     as u32,
    //                                 Some(ui_state.time_mode),
    //                             )?;
    //
    //                     // Override time parameters if we're in framed mode
    //                     if ui_state.davis_mode_radio_state == TranscoderMode::Framed {
    //                         davis_source = davis_source.time_parameters(
    //                             (255.0 * ui_state.davis_output_fps) as u32,
    //                             255,
    //                             255 * ui_state.delta_t_max_mult,
    //                             Some(ui_state.time_mode),
    //                         )?;
    //                     }
    //
    //                     if let Some(output_string) = output_string {
    //                         let writer = BufWriter::new(File::create(output_string)?);
    //                         davis_source = *davis_source.write_out(
    //                             DavisU8,
    //                             ui_state.time_mode,
    //                             ui_state.integration_mode_radio_state,
    //                             Some(ui_state.delta_t_max_mult as usize),
    //                             ui_state.encoder_type,
    //                             ui_state.encoder_options,
    //                             writer,
    //                         )?;
    //                     }
    //
    //                     Ok(AdderTranscoder {
    //                         framed_source: None,
    //                         davis_source: Some(davis_source),
    //                         prophesee_source: None,
    //                         live_image: Default::default(),
    //                     })
    //                 }
    //
    //                 // Prophesee .dat files
    //                 Some(ext) if ext == "dat" => {
    //                     let output_string = output_path_opt
    //                         .map(|output_path| output_path.to_str().expect("Bad path").to_string());
    //
    //                     let mut prophesee_source: Prophesee<BufWriter<File>> = Prophesee::new(
    //                         ui_state.delta_t_ref as u32,
    //                         input_path_buf.to_str().unwrap().to_string(),
    //                     )?
    //                     .crf(
    //                         ui_state
    //                             .encoder_options
    //                             .crf
    //                             .get_quality()
    //                             .unwrap_or(DEFAULT_CRF_QUALITY),
    //                     );
    //                     let adu_interval = (prophesee_source.get_video_ref().state.tps as f32
    //                         / ui_state.delta_t_ref as f32)
    //                         as usize;
    //
    //                     if let Some(output_string) = output_string {
    //                         let writer = BufWriter::new(File::create(output_string)?);
    //                         prophesee_source = *prophesee_source.write_out(
    //                             Dvs,
    //                             ui_state.time_mode,
    //                             ui_state.integration_mode_radio_state,
    //                             Some(adu_interval),
    //                             ui_state.encoder_type,
    //                             ui_state.encoder_options,
    //                             writer,
    //                         )?;
    //                     }
    //
    //                     ui_state.delta_t_max_mult =
    //                         prophesee_source.get_video_ref().get_delta_t_max()
    //                             / prophesee_source.get_video_ref().state.params.ref_time as u32;
    //                     // ui_state.delta_t_max_mult_slider = ui_state.delta_t_max_mult;
    //                     Ok(AdderTranscoder {
    //                         framed_source: None,
    //                         #[cfg(feature = "open-cv")]
    //                         davis_source: None,
    //                         prophesee_source: Some(prophesee_source),
    //                         // live_image: Default::default(),
    //                     })
    //                 }
    //
    //                 Some(_) => Err(Box::new(AdderTranscoderError("Invalid file type".into()))),
    //             }
    //         }
    //     }
    // }
}

// pub(crate) fn replace_adder_transcoder(
//     transcoder_state: &mut TranscoderState,
//     input_path_buf_0: Option<PathBuf>,
//     input_path_buf_1: Option<PathBuf>,
//     output_path_opt: Option<PathBuf>,
//     current_frame: u32,
// ) {
//     dbg!("Looping video");
//
//     let ui_info_state = &mut transcoder_state.ui_info_state;
//     ui_info_state.events_per_sec = 0.0;
//     ui_info_state.events_ppc_total = 0.0;
//     ui_info_state.events_total = 0;
//     ui_info_state.events_ppc_per_sec = 0.0;
//     ui_info_state.davis_latency = None;
//     if let Some(input_path) = input_path_buf_0 {
//         match AdderTranscoder::new(
//             &input_path,
//             &input_path_buf_1,
//             output_path_opt.clone(),
//             &mut transcoder_state.ui_state,
//             current_frame,
//         ) {
//             Ok(transcoder) => {
//                 transcoder_state.transcoder = transcoder;
//                 ui_info_state.source_name = egui::RichText::new(
//                     input_path
//                         .to_str()
//                         .unwrap_or("Error: invalid source string"),
//                 )
//                 .color(egui::Color32::DARK_GREEN);
//                 if let Some(output_path) = output_path_opt {
//                     ui_info_state.output_name.text = egui::RichText::new(
//                         output_path
//                             .to_str()
//                             .unwrap_or("Error: invalid output string"),
//                     )
//                     .color(egui::Color32::DARK_GREEN);
//                 }
//             }
//             Err(e) => {
//                 transcoder_state.transcoder = AdderTranscoder::default();
//                 ui_info_state.source_name = egui::RichText::new(e.to_string()).color(egui::Color32::RED);
//             }
//         };
//     } else {
//         eprintln!("No input path");
//     }
// }
