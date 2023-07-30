use crate::transcoder::adder::{replace_adder_transcoder, AdderTranscoder};
use crate::{slider_pm, Images};
use adder_codec_rs::transcoder::source::davis::TranscoderMode;
use adder_codec_rs::transcoder::source::video::{FramedViewMode, Source, SourceError};
use bevy::ecs::system::Resource;
use bevy::prelude::{Assets, Commands, Image, Res, ResMut, Time};
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy_egui::egui;
use bevy_egui::egui::{RichText, Ui};
use opencv::core::{Mat, MatTraitConstManual};
use opencv::imgproc;
use rayon::current_num_threads;
use std::error::Error;

use adder_codec_core::codec::EncoderType;
use adder_codec_core::TimeMode;
use adder_codec_rs::transcoder::source::davis::TranscoderMode::RawDvs;
use std::default::Default;
use std::fs::File;
use std::io::BufWriter;
use std::path::PathBuf;

pub struct ParamsUiState {
    pub(crate) delta_t_ref: f32,
    pub(crate) delta_t_ref_max: f32,
    pub(crate) delta_t_max_mult: u32,
    pub(crate) adder_tresh: f32,
    delta_t_ref_slider: f32,
    delta_t_max_mult_slider: u32,
    adder_tresh_slider: f32,
    pub(crate) scale: f64,
    scale_slider: f64,
    pub(crate) thread_count: usize,
    thread_count_slider: usize,
    pub(crate) color: bool,
    view_mode_radio_state: FramedViewMode,
    pub(crate) davis_mode_radio_state: TranscoderMode,
    pub(crate) davis_output_fps: f64,
    davis_output_fps_slider: f64,
    pub(crate) optimize_c: bool,
    pub(crate) optimize_c_frequency: u32,
    pub(crate) optimize_c_frequency_slider: u32,
    pub(crate) time_mode: TimeMode,
    pub(crate) encoder_type: EncoderType,
}

impl Default for ParamsUiState {
    fn default() -> Self {
        ParamsUiState {
            delta_t_ref: 255.0,
            delta_t_ref_max: 255.0,
            delta_t_max_mult: 120,
            adder_tresh: 10.0,
            delta_t_ref_slider: 255.0,
            delta_t_max_mult_slider: 120,
            adder_tresh_slider: 10.0,
            scale: 0.5,
            scale_slider: 0.5,
            thread_count: rayon::current_num_threads() - 1,
            thread_count_slider: rayon::current_num_threads() - 1,
            color: true,
            view_mode_radio_state: FramedViewMode::Intensity,
            davis_mode_radio_state: TranscoderMode::RawDavis,
            davis_output_fps: 500.0,
            davis_output_fps_slider: 500.0,
            optimize_c: true,
            optimize_c_frequency: 10,
            optimize_c_frequency_slider: 10,
            time_mode: TimeMode::default(),
            encoder_type: EncoderType::default(),
        }
    }
}

pub struct InfoUiState {
    pub events_per_sec: f64,
    pub events_ppc_per_sec: f64,
    pub events_ppc_total: f64,
    pub events_total: u64,
    pub source_name: RichText,
    pub output_name: OutputName,
    pub davis_latency: u128,
    pub(crate) input_path_0: Option<PathBuf>,
    pub(crate) input_path_1: Option<PathBuf>,
    pub(crate) output_path: Option<PathBuf>,
    pub view_mode_radio_state: FramedViewMode, // TODO: Move to different struct
}

pub struct OutputName {
    pub text: RichText,
}

impl Default for OutputName {
    fn default() -> Self {
        OutputName {
            text: RichText::new("No output selected yet"),
        }
    }
}

impl Default for InfoUiState {
    fn default() -> Self {
        InfoUiState {
            events_per_sec: 0.,
            events_ppc_per_sec: 0.,
            events_ppc_total: 0.0,
            events_total: 0,
            source_name: RichText::new("No input file selected yet"),
            output_name: Default::default(),
            davis_latency: 0,
            input_path_0: None,
            input_path_1: None,
            output_path: None,
            view_mode_radio_state: FramedViewMode::Intensity,
        }
    }
}

unsafe impl Sync for InfoUiState {}

#[derive(Resource, Default)]
pub struct TranscoderState {
    pub(crate) transcoder: AdderTranscoder,
    pub ui_state: ParamsUiState,
    pub ui_info_state: InfoUiState,
}

impl TranscoderState {
    pub fn side_panel_ui(
        &mut self,
        ui: &mut Ui,
        mut commands: Commands,
        _images: &mut ResMut<Assets<Image>>,
    ) {
        ui.horizontal(|ui| {
            ui.heading("ADΔER Parameters");
            if ui.add(egui::Button::new("Reset params")).clicked() {
                self.ui_state = Default::default();
            }
            if ui.add(egui::Button::new("Reset video")).clicked() {
                self.transcoder = AdderTranscoder::default();
                self.ui_info_state = InfoUiState::default();
                commands.insert_resource(Images::default());
            }
        });
        egui::Grid::new("my_grid")
            .num_columns(2)
            .spacing([10.0, 4.0])
            .striped(true)
            .show(ui, |ui| {
                side_panel_grid_contents(&self.transcoder, ui, &mut self.ui_state);
            });
    }

    pub fn central_panel_ui(&mut self, ui: &mut Ui, time: Res<Time>) {
        ui.horizontal(|ui| {
            if ui.button("Open file").clicked() {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("framed video", &["mp4"])
                    .add_filter("DVS/DAVIS video", &["aedat4"])
                    .pick_file()
                {
                    self.ui_info_state.input_path_0 = Some(path.clone());
                    self.ui_info_state.input_path_1 = None;
                    replace_adder_transcoder(
                        self,
                        Some(path),
                        None,
                        self.ui_info_state.output_path.clone(),
                        0,
                    );
                }
            }

            ui.label("OR drag and drop your source file here (.mp4, .aedat4)");
        });

        ui.horizontal(|ui| {
            if ui.button("Open DVS socket").clicked() {
                if let Some(path) = rfd::FileDialog::new()
                    .set_directory("/tmp")
                    .add_filter("DVS/DAVIS video", &["sock"])
                    .pick_file()
                {
                    self.ui_info_state.input_path_0 = Some(path.clone());
                }
            }
            if ui.button("Open APS socket").clicked() {
                if let Some(path) = rfd::FileDialog::new()
                    .set_directory("/tmp")
                    .add_filter("DVS/DAVIS video", &["sock"])
                    .pick_file()
                {
                    self.ui_info_state.input_path_1 = Some(path.clone());
                }
            }
            if ui.button("Go!").clicked() {
                if self.ui_info_state.input_path_0.is_some()
                    && self.ui_info_state.input_path_1.is_some()
                {
                    replace_adder_transcoder(
                        self,
                        self.ui_info_state.input_path_0.clone(),
                        self.ui_info_state.input_path_1.clone(),
                        self.ui_info_state.output_path.clone(),
                        0,
                    );
                }
            }
        });
        ui.label(self.ui_info_state.source_name.clone());

        if ui.button("Save file").clicked() {
            if let Some(mut path) = rfd::FileDialog::new()
                .add_filter("adder video", &["adder"])
                .save_file()
            {
                if !path.ends_with(".adder") {
                    path = path.with_extension("adder");
                };
                self.ui_info_state.output_path = Some(path.clone());
                self.ui_info_state.output_name = OutputName {
                    text: RichText::new(path.to_str().unwrap_or("Error: invalid output string")),
                };
                replace_adder_transcoder(
                    self,
                    self.ui_info_state.input_path_0.clone(),
                    self.ui_info_state.input_path_1.clone(),
                    Some(path),
                    0,
                );
            }
        }

        ui.label(self.ui_info_state.output_name.text.clone());

        ui.label(format!(
            "{:.2} transcoded FPS\t\
            {:.2} events per source sec\t\
            {:.2} events PPC per source sec\t\
            {:.0} events total\t\
            {:.0} events PPC total",
            1. / time.delta_seconds(),
            self.ui_info_state.events_per_sec,
            self.ui_info_state.events_ppc_per_sec,
            self.ui_info_state.events_total,
            self.ui_info_state.events_ppc_total
        ));

        if self.ui_info_state.davis_latency > 0 {
            ui.label(format!(
                "DAVIS/DVS latency: {:} ms",
                self.ui_info_state.davis_latency
            ));
        }
    }

    pub fn update_adder_params(&mut self) {
        // TODO: do conditionals on the sliders themselves

        let source: &mut dyn Source<BufWriter<File>> = {
            match &mut self.transcoder.framed_source {
                None => match &mut self.transcoder.davis_source {
                    None => {
                        return;
                    }
                    Some(source) => {
                        if source.mode != self.ui_state.davis_mode_radio_state
                            || source.get_reconstructor().as_ref().unwrap().output_fps
                                != self.ui_state.davis_output_fps
                            || source.time_mode != self.ui_state.time_mode
                            || source.get_video_ref().encoder_type != self.ui_state.encoder_type
                        {
                            if self.ui_state.davis_mode_radio_state == RawDvs {
                                // self.ui_state.davis_output_fps = 1000000.0;
                                // self.ui_state.davis_output_fps_slider = 1000000.0;
                                self.ui_state.optimize_c = false;
                            }
                            replace_adder_transcoder(
                                self,
                                self.ui_info_state.input_path_0.clone(),
                                self.ui_info_state.input_path_1.clone(),
                                self.ui_info_state.output_path.clone(),
                                0,
                            );
                            return;
                        }
                        let tmp = source.get_reconstructor_mut().as_mut().unwrap();
                        tmp.set_optimize_c(
                            self.ui_state.optimize_c,
                            self.ui_state.optimize_c_frequency,
                        );
                        source
                    }
                },
                Some(source) => {
                    if source.scale != self.ui_state.scale
                        || source.get_ref_time() != self.ui_state.delta_t_ref as u32
                        || source.time_mode != self.ui_state.time_mode
                        || source.get_video_ref().encoder_type != self.ui_state.encoder_type
                        || match source.get_video_ref().state.plane.c() {
                            1 => {
                                // True if the transcoder is gray, but the user wants color
                                self.ui_state.color
                            }
                            _ => {
                                // True if the transcoder is color, but the user wants gray
                                !self.ui_state.color
                            }
                        }
                    {
                        let current_frame =
                            source.get_video_ref().state.in_interval_count + source.frame_idx_start;
                        replace_adder_transcoder(
                            self,
                            self.ui_info_state.input_path_0.clone(),
                            self.ui_info_state.input_path_1.clone(),
                            self.ui_info_state.output_path.clone(),
                            current_frame,
                        );
                        return;
                    }
                    source
                }
            }
        };

        let video = source.get_video_mut();
        video.update_adder_thresh_pos(self.ui_state.adder_tresh as u8);
        // video.update_adder_thresh_neg(self.ui_state.adder_tresh as u8);
        video.update_delta_t_max(self.ui_state.delta_t_max_mult * video.get_ref_time());
        video.instantaneous_view_mode = self.ui_state.view_mode_radio_state;
    }

    pub fn consume_source(
        &mut self,
        mut images: ResMut<Assets<Image>>,
        mut handles: ResMut<Images>,
    ) -> Result<(), Box<dyn Error>> {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(self.ui_state.thread_count)
            .build()?;

        let mut ui_info_state = &mut self.ui_info_state;
        ui_info_state.events_per_sec = 0.;

        let source: &mut dyn Source<BufWriter<File>> = {
            match &mut self.transcoder.framed_source {
                None => match &mut self.transcoder.davis_source {
                    None => {
                        return Ok(());
                    }
                    Some(source) => {
                        ui_info_state.davis_latency = source.get_latency();
                        source
                    }
                },
                Some(source) => source,
            }
        };

        match source.consume(1, &pool) {
            Ok(events_vec_vec) => {
                for events_vec in events_vec_vec {
                    ui_info_state.events_total += events_vec.len() as u64;
                    ui_info_state.events_per_sec += events_vec.len() as f64;
                }
                ui_info_state.events_ppc_total = ui_info_state.events_total as f64
                    / (source.get_video_ref().state.plane.volume() as f64);
                let source_fps = source.get_video_ref().get_tps() as f64
                    / source.get_video_ref().get_ref_time() as f64;
                ui_info_state.events_per_sec *= source_fps;
                ui_info_state.events_ppc_per_sec = ui_info_state.events_per_sec
                    / (source.get_video_ref().state.plane.volume() as f64);
            }
            Err(SourceError::Open) => {}
            Err(e) => {
                eprintln!("Error: {:?}", e);
                source.get_video_mut().end_write_stream()?;
                self.ui_info_state.output_path = None;
                self.ui_info_state.output_name = Default::default();

                // Start video over from the beginning
                replace_adder_transcoder(
                    self,
                    self.ui_info_state.input_path_0.clone(),
                    self.ui_info_state.input_path_1.clone(),
                    None,
                    0,
                );
                return Ok(());
            }
        };

        let image_mat = &source.get_video_ref().instantaneous_frame;

        // add alpha channel
        let mut image_mat_bgra = Mat::default();
        imgproc::cvt_color(&image_mat, &mut image_mat_bgra, imgproc::COLOR_BGR2BGRA, 4)?;

        let image_bevy = Image::new(
            Extent3d {
                width: source.get_video_ref().state.plane.w().into(),
                height: source.get_video_ref().state.plane.h().into(),
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            Vec::from(image_mat_bgra.data_bytes()?),
            TextureFormat::Bgra8UnormSrgb,
        );
        self.transcoder.live_image = image_bevy;

        handles.last_image_view = handles.image_view.clone();
        let handle = images.add(self.transcoder.live_image.clone());
        handles.image_view = handle;
        Ok(())
    }
}

fn side_panel_grid_contents(
    transcoder: &AdderTranscoder,
    ui: &mut Ui,
    ui_state: &mut ParamsUiState,
) {
    let dtr_max = ui_state.delta_t_ref_max;
    let enabled = transcoder.davis_source.is_none();
    ui.add_enabled(enabled, egui::Label::new("Δt_ref:"));
    slider_pm(
        enabled,
        false,
        ui,
        &mut ui_state.delta_t_ref,
        &mut ui_state.delta_t_ref_slider,
        1.0..=dtr_max,
        vec![],
        10.0,
    );
    ui.end_row();

    ui.label("Δt_max multiplier:");
    slider_pm(
        true,
        false,
        ui,
        &mut ui_state.delta_t_max_mult,
        &mut ui_state.delta_t_max_mult_slider,
        1..=1000,
        vec![],
        10,
    );
    ui.end_row();

    ui.label("ADΔER threshold:");
    slider_pm(
        true,
        false,
        ui,
        &mut ui_state.adder_tresh,
        &mut ui_state.adder_tresh_slider,
        0.0..=255.0,
        vec![],
        1.0,
    );
    ui.end_row();

    ui.label("Thread count:");
    slider_pm(
        true,
        false,
        ui,
        &mut ui_state.thread_count,
        &mut ui_state.thread_count_slider,
        1..=(current_num_threads() - 1).max(4),
        vec![],
        1,
    );
    ui.end_row();

    ui.label("Video scale:");
    slider_pm(
        enabled,
        false,
        ui,
        &mut ui_state.scale,
        &mut ui_state.scale_slider,
        0.001..=1.0,
        vec![0.25, 0.5, 0.75],
        0.1,
    );
    ui.end_row();

    ui.label("Channels:");
    ui.add_enabled(enabled, egui::Checkbox::new(&mut ui_state.color, "Color?"));
    ui.end_row();

    ui.label("View mode:");
    ui.horizontal(|ui| {
        ui.radio_value(
            &mut ui_state.view_mode_radio_state,
            FramedViewMode::Intensity,
            "Intensity",
        );
        ui.radio_value(&mut ui_state.view_mode_radio_state, FramedViewMode::D, "D");
        ui.radio_value(
            &mut ui_state.view_mode_radio_state,
            FramedViewMode::DeltaT,
            "Δt",
        );
        ui.radio_value(
            &mut ui_state.view_mode_radio_state,
            FramedViewMode::SAE,
            "SAE",
        );
    });
    ui.end_row();

    ui.label("Time mode:");
    ui.add_enabled_ui(true, |ui| {
        ui.horizontal(|ui| {
            ui.radio_value(
                &mut ui_state.time_mode,
                TimeMode::DeltaT,
                "Δt (time change)",
            );
            ui.radio_value(
                &mut ui_state.time_mode,
                TimeMode::AbsoluteT,
                "t (absolute time)",
            );
        });
    });
    ui.end_row();

    ui.label("Compression mode:");
    ui.add_enabled_ui(true, |ui| {
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                ui.radio_value(
                    &mut ui_state.encoder_type,
                    EncoderType::Empty,
                    "Empty (don't write)",
                );
                ui.radio_value(&mut ui_state.encoder_type, EncoderType::Raw, "Raw");
            });
            ui.horizontal(|ui| {
                ui.radio_value(
                    &mut ui_state.encoder_type,
                    EncoderType::RawInterleaved,
                    "Raw, temporally interleaved",
                );
                ui.radio_value(
                    &mut ui_state.encoder_type,
                    EncoderType::Compressed,
                    "Compressed",
                );
            });
        });
    });
    ui.end_row();

    ui.label("DAVIS mode:");
    ui.add_enabled_ui(!enabled, |ui| {
        ui.horizontal(|ui| {
            ui.radio_value(
                &mut ui_state.davis_mode_radio_state,
                TranscoderMode::Framed,
                "Framed recon",
            );
            ui.radio_value(
                &mut ui_state.davis_mode_radio_state,
                TranscoderMode::RawDavis,
                "Raw DAVIS",
            );
            ui.radio_value(
                &mut ui_state.davis_mode_radio_state,
                TranscoderMode::RawDvs,
                "Raw DVS",
            );
        });
    });
    ui.end_row();

    ui.label("DAVIS deblurred FPS:");

    slider_pm(
        !enabled,
        true,
        ui,
        &mut ui_state.davis_output_fps,
        &mut ui_state.davis_output_fps_slider,
        30.0..=1000000.0,
        vec![
            50.0, 100.0, 250.0, 500.0, 1_000.0, 2_500.0, 5_000.0, 7_500.0, 10_000.0, 1000000.0,
        ],
        50.0,
    );
    ui.end_row();

    let enable_optimize = !enabled && ui_state.davis_mode_radio_state != TranscoderMode::RawDvs;
    ui.label("Optimize:");
    ui.add_enabled(
        enable_optimize,
        egui::Checkbox::new(&mut ui_state.optimize_c, "Optimize θ?"),
    );
    ui.end_row();

    ui.label("Optimize frequency:");
    slider_pm(
        enable_optimize,
        true,
        ui,
        &mut ui_state.optimize_c_frequency,
        &mut ui_state.optimize_c_frequency_slider,
        1..=250,
        vec![10, 25, 50, 100],
        1,
    );
    ui.end_row();
}
