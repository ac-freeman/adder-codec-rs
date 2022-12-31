use std::error::Error;

use adder_codec_rs::transcoder::source::davis_source::DavisSource;
use adder_codec_rs::transcoder::source::framed_source::FramedSource;
use adder_codec_rs::transcoder::source::framed_source::FramedSourceBuilder;
use adder_codec_rs::SourceCamera;
use bevy::prelude::Image;
use std::fmt;
use std::path::{Path, PathBuf};

use adder_codec_rs::transcoder::source::davis_source::DavisTranscoderMode;

use adder_codec_rs::aedat::base::ioheader_generated::Compression;
use adder_codec_rs::davis_edi_rs::util::reconstructor::Reconstructor;

use crate::transcoder::ui::{ParamsUiState, TranscoderState};
use bevy_egui::egui::{Color32, RichText};
use opencv::Result;

#[derive(Default)]
pub struct AdderTranscoder {
    pub(crate) framed_source: Option<FramedSource>,
    pub(crate) davis_source: Option<DavisSource>,
    pub(crate) live_image: Image,
}

#[derive(Debug)]
struct AdderTranscoderError(String);

impl fmt::Display for AdderTranscoderError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "ADDER transcoder: {}", self.0)
    }
}

impl Error for AdderTranscoderError {}

impl AdderTranscoder {
    pub(crate) fn new(
        input_path_buf: &Path,
        output_path_opt: Option<PathBuf>,
        ui_state: &mut ParamsUiState,
        current_frame: u32,
    ) -> Result<Self, Box<dyn Error>> {
        match input_path_buf.extension() {
            None => Err(Box::new(AdderTranscoderError("Invalid file type".into()))),
            Some(ext) => {
                match ext.to_str() {
                    None => Err(Box::new(AdderTranscoderError("Invalid file type".into()))),
                    Some("mp4") => {
                        let mut builder = FramedSourceBuilder::new(
                            input_path_buf.to_str().unwrap().to_string(),
                            SourceCamera::FramedU8,
                        )
                        .frame_start(current_frame)
                        .chunk_rows(64)
                        .scale(ui_state.scale)
                        .communicate_events(true)
                        .color(ui_state.color)
                        .contrast_thresholds(ui_state.adder_tresh as u8, ui_state.adder_tresh as u8)
                        .show_display(false)
                        .time_parameters(
                            ui_state.delta_t_ref as u32,
                            ui_state.delta_t_max_mult * ui_state.delta_t_ref as u32,
                        );

                        match output_path_opt {
                            None => {}
                            Some(output_path) => {
                                builder = builder.output_events_filename(
                                    output_path.to_str().unwrap().parse().unwrap(),
                                );
                            }
                        };

                        match builder.finish() {
                            Ok(source) => {
                                ui_state.delta_t_ref_max = 255.0;
                                Ok(AdderTranscoder {
                                    framed_source: Some(source),
                                    davis_source: None,
                                    live_image: Default::default(),
                                })
                            }
                            Err(_e) => {
                                Err(Box::new(AdderTranscoderError("Invalid file type".into())))
                            }
                        }
                    }
                    Some("aedat4") => {
                        let events_only = match &ui_state.davis_mode_radio_state {
                            DavisTranscoderMode::Framed => false,
                            DavisTranscoderMode::RawDavis => false,
                            DavisTranscoderMode::RawDvs => true,
                        };
                        let deblur_only = match &ui_state.davis_mode_radio_state {
                            DavisTranscoderMode::Framed => false,
                            DavisTranscoderMode::RawDavis => true,
                            DavisTranscoderMode::RawDvs => true,
                        };

                        let rt = tokio::runtime::Builder::new_multi_thread()
                            .worker_threads(ui_state.thread_count)
                            .enable_time()
                            .build()
                            .unwrap();
                        let dir = input_path_buf
                            .parent()
                            .expect("File must be in some directory")
                            .to_str()
                            .expect("Bad path")
                            .to_string();
                        let filename = input_path_buf
                            .file_name()
                            .expect("File must exist")
                            .to_str()
                            .expect("Bad filename")
                            .to_string();
                        eprintln!("{}", filename);
                        let reconstructor = rt.block_on(Reconstructor::new(
                            dir + "/",
                            filename,
                            "".to_string(),
                            "file".to_string(), // TODO
                            0.15,
                            ui_state.optimize_c,
                            false,
                            false,
                            false,
                            ui_state.davis_output_fps,
                            Compression::None,
                            346,
                            260,
                            deblur_only,
                            events_only,
                            1000.0, // Target latency (not used)
                            true,
                        ));

                        let output_string = output_path_opt
                            .map(|output_path| output_path.to_str().unwrap().to_string());

                        let davis_source = DavisSource::new(
                            reconstructor,
                            output_string.clone(),
                            1000000_u32, // TODO
                            1000000.0 / ui_state.davis_output_fps,
                            (1000000.0 * ui_state.delta_t_max_mult as f32) as u32, // TODO
                            false,
                            ui_state.adder_tresh as u8,
                            ui_state.adder_tresh as u8,
                            false,
                            rt,
                            ui_state.davis_mode_radio_state,
                            output_string.is_some(),
                        )
                        .unwrap();

                        Ok(AdderTranscoder {
                            framed_source: None,
                            davis_source: Some(davis_source),
                            live_image: Default::default(),
                        })
                    }
                    Some(_) => Err(Box::new(AdderTranscoderError("Invalid file type".into()))),
                }
            }
        }
    }
}

pub(crate) fn replace_adder_transcoder(
    transcoder_state: &mut TranscoderState,
    input_path_buf: Option<PathBuf>,
    output_path_opt: Option<PathBuf>,
    current_frame: u32,
) {
    let mut ui_info_state = &mut transcoder_state.ui_info_state;
    ui_info_state.events_per_sec = 0.0;
    ui_info_state.events_ppc_total = 0.0;
    ui_info_state.events_total = 0;
    ui_info_state.events_ppc_per_sec = 0.0;
    if let Some(input_path) = input_path_buf {
        match AdderTranscoder::new(
            &input_path,
            output_path_opt.clone(),
            &mut transcoder_state.ui_state,
            current_frame,
        ) {
            Ok(transcoder) => {
                transcoder_state.transcoder = transcoder;
                ui_info_state.source_name =
                    RichText::new(input_path.to_str().unwrap()).color(Color32::DARK_GREEN);
                if let Some(output_path) = output_path_opt {
                    ui_info_state.output_name =
                        RichText::new(output_path.to_str().unwrap()).color(Color32::DARK_GREEN);
                }
            }
            Err(e) => {
                transcoder_state.transcoder = AdderTranscoder::default();
                ui_info_state.source_name = RichText::new(e.to_string()).color(Color32::RED);
            }
        };
    } else {
    }
}
