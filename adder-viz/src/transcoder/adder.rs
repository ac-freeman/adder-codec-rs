use std::error::Error;

#[cfg(feature = "open-cv")]
use adder_codec_rs::transcoder::source::davis::Davis;
use adder_codec_rs::transcoder::source::framed::Framed;
use bevy::prelude::Image;
use std::fmt;
use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};

#[cfg(feature = "open-cv")]
use adder_codec_rs::transcoder::source::davis::TranscoderMode;

#[cfg(feature = "open-cv")]
use adder_codec_rs::davis_edi_rs::util::reconstructor::Reconstructor;

use crate::transcoder::ui::{ParamsUiState, TranscoderState};
use adder_codec_rs::adder_codec_core::codec::rate_controller::DEFAULT_CRF_QUALITY;
use adder_codec_rs::adder_codec_core::SourceCamera::{DavisU8, Dvs, FramedU8};
use adder_codec_rs::transcoder::source::prophesee::Prophesee;
use adder_codec_rs::transcoder::source::video::{Source, VideoBuilder};
use bevy_egui::egui::{Color32, RichText};
#[cfg(feature = "open-cv")]
use opencv::Result;

#[derive(Default)]
pub struct AdderTranscoder {
    pub(crate) framed_source: Option<Framed<BufWriter<File>>>,
    #[cfg(feature = "open-cv")]
    pub(crate) davis_source: Option<Davis<BufWriter<File>>>,
    pub(crate) prophesee_source: Option<Prophesee<BufWriter<File>>>,
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
        _input_path_buf_1: &Option<PathBuf>,
        output_path_opt: Option<PathBuf>,
        ui_state: &mut ParamsUiState,
        current_frame: u32,
    ) -> Result<Self, Box<dyn Error>> {
        match input_path_buf.extension() {
            None => Err(Box::new(AdderTranscoderError("Invalid file type".into()))),
            Some(ext) => {
                match ext.to_ascii_lowercase().to_str() {
                    None => Err(Box::new(AdderTranscoderError("Invalid file type".into()))),
                    Some("mp4") => {
                        let mut framed: Framed<BufWriter<File>> = Framed::new(
                            match input_path_buf.to_str() {
                                None => {
                                    return Err(Box::new(AdderTranscoderError(
                                        "Couldn't get input path string".into(),
                                    )))
                                }
                                Some(path) => path.to_string(),
                            },
                            ui_state.color,
                            ui_state.scale,
                        )?
                        .crf(
                            ui_state
                                .encoder_options
                                .crf
                                .get_quality()
                                .unwrap_or(DEFAULT_CRF_QUALITY),
                        )
                        .frame_start(current_frame)?
                        .chunk_rows(1)
                        .auto_time_parameters(
                            ui_state.delta_t_ref as u32,
                            ui_state.delta_t_max_mult * ui_state.delta_t_ref as u32,
                            Some(ui_state.time_mode),
                        )?;

                        // TODO: Change the builder to take in a pathbuf directly, not a string,
                        // and to handle the error checking in the associated function
                        match output_path_opt {
                            None => {}
                            Some(output_path) => {
                                let out_path = output_path.to_str().unwrap();
                                let writer = BufWriter::new(File::create(out_path)?);

                                framed = *framed.write_out(
                                    FramedU8,
                                    ui_state.time_mode,
                                    ui_state.integration_mode_radio_state,
                                    Some(ui_state.delta_t_max_mult as usize),
                                    ui_state.encoder_type,
                                    ui_state.encoder_options,
                                    writer,
                                )?;
                            }
                        };

                        ui_state.delta_t_ref_max = 255.0;
                        Ok(AdderTranscoder {
                            framed_source: Some(framed),
                            #[cfg(feature = "open-cv")]
                            davis_source: None,
                            prophesee_source: None,
                            live_image: Default::default(),
                        })
                        // }
                        // Err(_e) => {
                        //     Err(Box::new(AdderTranscoderError("Invalid file type".into())))
                        // }
                    }

                    #[cfg(feature = "open-cv")]
                    Some(ext) if ext == "aedat4" || ext == "sock" => {
                        let events_only = match &ui_state.davis_mode_radio_state {
                            TranscoderMode::Framed => false,
                            TranscoderMode::RawDavis => false,
                            TranscoderMode::RawDvs => true,
                        };
                        let deblur_only = match &ui_state.davis_mode_radio_state {
                            TranscoderMode::Framed => false,
                            TranscoderMode::RawDavis => true,
                            TranscoderMode::RawDvs => true,
                        };

                        let rt = tokio::runtime::Builder::new_multi_thread()
                            .worker_threads(ui_state.thread_count)
                            .enable_time()
                            .build()?;
                        let dir = input_path_buf
                            .parent()
                            .expect("File must be in some directory")
                            .to_str()
                            .expect("Bad path")
                            .to_string();
                        let filename_0 = input_path_buf
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

                        let filename_1 = _input_path_buf_1.as_ref().map(|_input_path_buf_1| {
                            _input_path_buf_1
                                .file_name()
                                .expect("File must exist")
                                .to_str()
                                .expect("Bad filename")
                                .to_string()
                        });

                        let reconstructor = rt.block_on(Reconstructor::new(
                            dir + "/",
                            filename_0,
                            filename_1.unwrap_or("".to_string()),
                            mode.to_string(), // TODO
                            0.15,
                            ui_state.optimize_c,
                            ui_state.optimize_c_frequency,
                            false,
                            false,
                            false,
                            ui_state.davis_output_fps,
                            deblur_only,
                            events_only,
                            1000.0, // Target latency (not used)
                            simulate_latency,
                        ))?;

                        let output_string = output_path_opt
                            .map(|output_path| output_path.to_str().expect("Bad path").to_string());

                        let mut davis_source: Davis<BufWriter<File>> =
                            Davis::new(reconstructor, rt, ui_state.davis_mode_radio_state)?
                                .optimize_adder_controller(false) // TODO
                                .mode(ui_state.davis_mode_radio_state)
                                .crf(
                                    ui_state
                                        .encoder_options
                                        .crf
                                        .get_quality()
                                        .unwrap_or(DEFAULT_CRF_QUALITY),
                                )
                                .time_parameters(
                                    1000000_u32,
                                    (1_000_000.0 / ui_state.davis_output_fps) as adder_codec_rs::adder_codec_core::DeltaT,
                                    ((1_000_000.0 / ui_state.davis_output_fps)
                                        * ui_state.delta_t_max_mult as f64)
                                        as u32,
                                    Some(ui_state.time_mode),
                                )?;

                        // Override time parameters if we're in framed mode
                        if ui_state.davis_mode_radio_state == TranscoderMode::Framed {
                            davis_source = davis_source.time_parameters(
                                (255.0 * ui_state.davis_output_fps) as u32,
                                255,
                                255 * ui_state.delta_t_max_mult,
                                Some(ui_state.time_mode),
                            )?;
                        }

                        if let Some(output_string) = output_string {
                            let writer = BufWriter::new(File::create(output_string)?);
                            davis_source = *davis_source.write_out(
                                DavisU8,
                                ui_state.time_mode,
                                ui_state.integration_mode_radio_state,
                                Some(ui_state.delta_t_max_mult as usize),
                                ui_state.encoder_type,
                                ui_state.encoder_options,
                                writer,
                            )?;
                        }

                        Ok(AdderTranscoder {
                            framed_source: None,
                            davis_source: Some(davis_source),
                            prophesee_source: None,
                            live_image: Default::default(),
                        })
                    }

                    // Prophesee .dat files
                    Some(ext) if ext == "dat" => {
                        let output_string = output_path_opt
                            .map(|output_path| output_path.to_str().expect("Bad path").to_string());

                        let mut prophesee_source: Prophesee<BufWriter<File>> = Prophesee::new(
                            ui_state.delta_t_ref as u32,
                            input_path_buf.to_str().unwrap().to_string(),
                        )?
                        .crf(
                            ui_state
                                .encoder_options
                                .crf
                                .get_quality()
                                .unwrap_or(DEFAULT_CRF_QUALITY),
                        );
                        let adu_interval = (prophesee_source.get_video_ref().state.tps as f32
                            / ui_state.delta_t_ref)
                            as usize;

                        if let Some(output_string) = output_string {
                            let writer = BufWriter::new(File::create(output_string)?);
                            prophesee_source = *prophesee_source.write_out(
                                Dvs,
                                ui_state.time_mode,
                                ui_state.integration_mode_radio_state,
                                Some(adu_interval),
                                ui_state.encoder_type,
                                ui_state.encoder_options,
                                writer,
                            )?;
                        }

                        ui_state.delta_t_max_mult =
                            prophesee_source.get_video_ref().get_delta_t_max()
                                / prophesee_source.get_video_ref().state.params.ref_time as u32;
                        ui_state.delta_t_max_mult_slider = ui_state.delta_t_max_mult;
                        Ok(AdderTranscoder {
                            framed_source: None,
                            #[cfg(feature = "open-cv")]
                            davis_source: None,
                            prophesee_source: Some(prophesee_source),
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
    input_path_buf_0: Option<PathBuf>,
    input_path_buf_1: Option<PathBuf>,
    output_path_opt: Option<PathBuf>,
    current_frame: u32,
) {
    dbg!("Looping video");

    let ui_info_state = &mut transcoder_state.ui_info_state;
    ui_info_state.events_per_sec = 0.0;
    ui_info_state.events_ppc_total = 0.0;
    ui_info_state.events_total = 0;
    ui_info_state.events_ppc_per_sec = 0.0;
    ui_info_state.davis_latency = None;
    if let Some(input_path) = input_path_buf_0 {
        match AdderTranscoder::new(
            &input_path,
            &input_path_buf_1,
            output_path_opt.clone(),
            &mut transcoder_state.ui_state,
            current_frame,
        ) {
            Ok(transcoder) => {
                transcoder_state.transcoder = transcoder;
                ui_info_state.source_name = RichText::new(
                    input_path
                        .to_str()
                        .unwrap_or("Error: invalid source string"),
                )
                .color(Color32::DARK_GREEN);
                if let Some(output_path) = output_path_opt {
                    ui_info_state.output_name.text = RichText::new(
                        output_path
                            .to_str()
                            .unwrap_or("Error: invalid output string"),
                    )
                    .color(Color32::DARK_GREEN);
                }
            }
            Err(e) => {
                transcoder_state.transcoder = AdderTranscoder::default();
                ui_info_state.source_name = RichText::new(e.to_string()).color(Color32::RED);
            }
        };
    } else {
        eprintln!("No input path");
    }
}
