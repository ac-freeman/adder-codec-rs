use adder_codec_rs::adder_codec_core::codec::rate_controller::{Crf, CRF, DEFAULT_CRF_QUALITY};
use adder_codec_rs::adder_codec_core::codec::{EncoderOptions, EncoderType, EventDrop, EventOrder};
use adder_codec_rs::adder_codec_core::{PixelMultiMode, PlaneSize, TimeMode};
#[cfg(feature = "open-cv")]
use adder_codec_rs::transcoder::source::davis::TranscoderMode;
use adder_codec_rs::transcoder::source::video::FramedViewMode;
use adder_codec_rs::utils::cv::QualityMetrics;
use adder_codec_rs::utils::viz::ShowFeatureMode;
use eframe::epaint::{ColorImage, ImageDelta};
use egui::epaint::TextureManager;
use egui::{ImageSource, TextureOptions, Vec2b};
use egui_plot::Corner::LeftTop;
use egui_plot::{Legend, Plot};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use tokio::sync::mpsc::error::TryRecvError;
use tokio::sync::mpsc::Sender;
// use crate::transcoder::adder::{replace_adder_transcoder, AdderTranscoder};
// use crate::utils::prep_bevy_image;
use crate::transcoder::adder::AdderTranscoder;
use crate::transcoder::{AdaptiveParams, CoreParams, EventRateMsg, InfoParams, InfoUiState};
use crate::{App, Images, TabState, Tabs};
// #[cfg(feature = "open-cv")]
// use adder_codec_rs::transcoder::source::davis::TranscoderMode;
// use adder_codec_rs::transcoder::source::video::{FramedViewMode, Source, SourceError};
// use rayon::current_num_threads;
// use std::collections::VecDeque;
// use std::error::Error;
//
use crate::utils::{slider_pm, PlotY};
// use adder_codec_rs::adder_codec_core::codec::rate_controller::{Crf, CRF, DEFAULT_CRF_QUALITY};
// use adder_codec_rs::adder_codec_core::codec::{EncoderOptions, EncoderType, EventDrop, EventOrder};
// use adder_codec_rs::adder_codec_core::TimeMode;
// use adder_codec_rs::adder_codec_core::{PixelMultiMode, PlaneSize};
// #[cfg(feature = "open-cv")]
// use adder_codec_rs::transcoder::source::davis::TranscoderMode::RawDvs;
// use adder_codec_rs::utils::cv::{calculate_quality_metrics, QualityMetrics};
// use adder_codec_rs::utils::viz::ShowFeatureMode;
// use std::default::Default;
// use std::fs::File;
// use std::io::{BufWriter, Write};
// use std::path::PathBuf;
//

//

//
// pub struct OutputName {
//     pub text: RichText,
// }
//
// impl Default for OutputName {
//     fn default() -> Self {
//         OutputName {
//             text: RichText::new("No output selected yet"),
//         }
//     }
// }
//
// impl Default for InfoUiState {
//     fn default() -> Self {
//         let plot_points: VecDeque<Option<f64>> = (0..1000).map(|_| None).collect();
//
//         InfoUiState {
//             events_per_sec: 0.,
//             events_ppc_per_sec: 0.,
//             events_ppc_total: 0.0,
//             events_total: 0,
//             event_size: 0,
//             source_samples_per_sec: 0.0,
//             plane: Default::default(),
//             source_name: RichText::new("No input file selected yet"),
//             output_name: Default::default(),
//             davis_latency: None,
//             input_path_0: None,
//             input_path_1: None,
//             output_path: None,
//             plot_points_eventrate_y: PlotY {
//                 points: plot_points.clone(),
//             },
//             plot_points_raw_adder_bitrate_y: PlotY {
//                 points: plot_points.clone(),
//             },
//             plot_points_raw_source_bitrate_y: PlotY {
//                 points: plot_points.clone(),
//             },
//             plot_points_psnr_y: PlotY {
//                 points: plot_points.clone(),
//             },
//             plot_points_mse_y: PlotY {
//                 points: plot_points.clone(),
//             },
//             plot_points_ssim_y: PlotY {
//                 points: plot_points.clone(),
//             },
//             plot_points_latency_y: PlotY {
//                 points: plot_points,
//             },
//             view_mode_radio_state: FramedViewMode::Intensity,
//         }
//     }
// }
//
// unsafe impl Sync for InfoUiState {}
//
#[derive(Default, Debug, Clone, PartialEq)]
pub struct TranscoderState {
    pub adaptive_params: AdaptiveParams,
    pub core_params: CoreParams,
    pub info_params: InfoParams,
}

impl TabState for TranscoderState {
    fn reset_params(&mut self) {
        self.adaptive_params = Default::default();
        let input_path_buf_0 = self.core_params.input_path_buf_0.clone();
        self.core_params = Default::default();
        self.core_params.input_path_buf_0 = input_path_buf_0;
    }

    fn reset_video(&mut self) {
        self.core_params.input_path_buf_0 = None;
        self.core_params.output_path = None;
    }
}

#[derive(Debug, Clone)]
pub enum TranscoderStateMsg {
    Terminate,
    Set { transcoder_state: TranscoderState },
}

#[derive(Debug, Clone)]
pub enum TranscoderInfoMsg {
    Plane((PlaneSize, bool)),
    QualityMetrics(QualityMetrics),
    EventRateMsg(EventRateMsg),
    Error(String),
}

pub struct TranscoderUi {
    pub transcoder_state: TranscoderState,
    pub transcoder_state_last_sent: TranscoderState,
    pub info_ui_state: InfoUiState,
    msg_rx: mpsc::Receiver<TranscoderInfoMsg>,
    pub transcoder_state_tx: Sender<TranscoderStateMsg>,
    adder_image_handle: egui::TextureHandle,
    input_image_handle: egui::TextureHandle,
    last_frame_time: std::time::Instant,
    slider_button_down: bool,
}

impl TranscoderUi {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let (tx, rx) = mpsc::channel(5);
        let (msg_tx, msg_rx) = mpsc::channel(30);

        let mut transcoder_ui = TranscoderUi {
            transcoder_state: Default::default(),
            transcoder_state_last_sent: Default::default(),
            info_ui_state: InfoUiState::default(),
            msg_rx,
            transcoder_state_tx: tx,
            adder_image_handle: cc.egui_ctx.load_texture(
                "adder_image",
                ColorImage::default(),
                Default::default(),
            ),
            input_image_handle: cc.egui_ctx.load_texture(
                "adder_image",
                ColorImage::default(),
                Default::default(),
            ),
            last_frame_time: std::time::Instant::now(),
            slider_button_down: false,
        };
        transcoder_ui.spawn_transcoder(rx, msg_tx);
        transcoder_ui
    }

    fn spawn_transcoder(
        &mut self,
        rx: mpsc::Receiver<TranscoderStateMsg>,
        msg_tx: mpsc::Sender<TranscoderInfoMsg>,
    ) {
        let adder_image_handle = self.adder_image_handle.clone();
        let input_image_handle = self.input_image_handle.clone();
        let rt = tokio::runtime::Runtime::new().expect("Unable to create Runtime");

        let _enter = rt.enter();

        // Execute the runtime in its own thread.
        std::thread::spawn(move || {
            rt.block_on(async {
                let mut transcoder =
                    AdderTranscoder::new(rx, msg_tx, input_image_handle, adder_image_handle);
                transcoder.run().await;
            })
        });
    }
    pub fn update(&mut self, ctx: &egui::Context) {
        // Store a copy of the params to compare against later
        let old_params = self.transcoder_state_last_sent.clone();

        // Collect dropped files
        self.handle_file_drop(ctx);

        self.handle_info_messages();

        self.draw_ui(ctx);

        // This should always be the very last thing we do in this function
        if old_params != self.transcoder_state && !self.slider_button_down {
            self.transcoder_state_tx
                .blocking_send(TranscoderStateMsg::Set {
                    transcoder_state: self.transcoder_state.clone(),
                })
                .unwrap();
            self.transcoder_state_last_sent = self.transcoder_state.clone();
        }
    }

    fn handle_info_messages(&mut self) {
        loop {
            match self.msg_rx.try_recv() {
                Ok(message) => match message {
                    TranscoderInfoMsg::QualityMetrics(metrics) => self.handle_metrics(metrics),
                    TranscoderInfoMsg::Error(error_string) => {
                        self.info_ui_state.error_string = Some(error_string);
                    }
                    TranscoderInfoMsg::EventRateMsg(msg) => {
                        self.info_ui_state.total_events = msg.total_events;
                        self.info_ui_state.events_per_sec = msg.events_per_sec;
                        self.info_ui_state.events_ppc_total = msg.events_ppc_total;
                        self.info_ui_state.events_ppc_per_sec = msg.events_ppc_per_sec;
                        self.info_ui_state.transcoded_fps = msg.transcoded_fps;

                        #[rustfmt::skip]
                        let event_size = if self.transcoder_state.core_params.color { 11} else {9};

                        let bitrate = self.info_ui_state.events_ppc_per_sec
                            * event_size as f64
                            * msg.num_pixels as f64
                            / 1024.0
                            / 1024.0; // transcoded raw in megabytes per sec
                        self.info_ui_state
                            .plot_points_raw_adder_bitrate_y
                            .update(Some(bitrate));

                        let raw_source_bitrate = msg.running_input_bitrate / 8.0 / 1024.0 / 1024.0; // source in megabytes per sec
                        self.info_ui_state
                            .plot_points_raw_source_bitrate_y
                            .update(Some(raw_source_bitrate));
                    }
                    TranscoderInfoMsg::Plane((plane, finish)) => {
                        // Received when we have created a new video
                        if finish {
                            self.transcoder_state.core_params.output_path = None;
                        }
                        self.transcoder_state
                            .adaptive_params
                            .encoder_options
                            .crf
                            .plane = plane;
                        if self.transcoder_state.adaptive_params.auto_quality {
                            self.transcoder_state
                                .adaptive_params
                                .encoder_options
                                .crf
                                .update_quality(self.transcoder_state.adaptive_params.crf_number);
                        }
                    }
                },
                Err(_) => {
                    break;
                }
            }
        }
    }

    fn handle_metrics(&mut self, metrics: QualityMetrics) {
        self.info_ui_state.plot_points_psnr_y.update(metrics.psnr);
        self.info_ui_state.plot_points_mse_y.update(metrics.mse);
        self.info_ui_state.plot_points_ssim_y.update(metrics.ssim);
    }

    /// If the user has dropped a file into the window, we store the file path.
    /// At the end of the frame, the receiver will be notified by update()
    fn handle_file_drop(&mut self, ctx: &egui::Context) {
        ctx.input(|i| {
            if !i.raw.dropped_files.is_empty() {
                self.transcoder_state.core_params.input_path_buf_0 =
                    i.raw.dropped_files[0].path.clone();
            }
        });
    }

    fn draw_ui(
        &mut self,
        ctx: &egui::Context, // mut transcoder_state: ResMut<TranscoderState>,
                             // mut player_state: ResMut<PlayerState>,
                             // main_ui_state: Res<MainUiState>,
    ) {
        egui::SidePanel::left("side_panel")
            .default_width(300.0)
            .show(ctx, |ui| {
                ui.label(format!(
                    "FPS: {:.2}",
                    1.0 / self.last_frame_time.elapsed().as_secs_f64()
                ));
                // update the last frame time
                self.last_frame_time = std::time::Instant::now();

                self.side_panel_ui(ui);
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            egui::warn_if_debug_build(ui);

            self.central_panel_ui(ui);
        });
    }

    fn side_panel_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("ADΔER Parameters");
            if ui.add(egui::Button::new("Reset params")).clicked() {
                self.transcoder_state.reset_params();
            }
            if ui.add(egui::Button::new("Reset video")).clicked() {
                self.transcoder_state.reset_video();
                self.transcoder_state_tx
                    .blocking_send(TranscoderStateMsg::Terminate)
                    .unwrap();
                // if let Some(framed_source) = &mut self.transcoder.framed_source {
                //     match framed_source.get_video_mut().end_write_stream() {
                //         Ok(Some(mut writer)) => {
                //             writer.flush().unwrap();
                //         }
                //         Ok(None) => {}
                //         Err(_) => {}
                //     }
                // }

                // self.transcoder = AdderTranscoder::default();
                // self.ui_info_state = InfoUiState::default();
                // commands.insert_resource(Images::default());
            }
        });
        egui::Grid::new("my_grid")
            .num_columns(2)
            .spacing([10.0, 4.0])
            .striped(true)
            .show(ui, |ui| {
                self.side_panel_grid_contents(ui);
            });
    }
    //
    pub fn central_panel_ui(&mut self, ui: &mut egui::Ui) {
        ui.colored_label(
            ui.visuals().warn_fg_color,
            self.info_ui_state
                .error_string
                .as_ref()
                .unwrap_or(&String::new()),
        );
        ui.horizontal(|ui| {
            if ui.button("Open file").clicked() {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("framed video", &["mp4", "mkv", "avi", "mov"])
                    .add_filter("DVS/DAVIS video", &["aedat4"])
                    .add_filter("Prophesee video", &["dat"])
                    .pick_file()
                {
                    eprintln!("Updating input path: {path:?}");
                    self.transcoder_state.core_params.input_path_buf_0 = Some(path.clone());
                }
            }
            let label_opt = &self.transcoder_state.core_params.input_path_buf_0;
            ui.colored_label(
                if label_opt.is_some() {
                    egui::Color32::GREEN
                } else {
                    ui.style().visuals.text_color()
                },
                label_opt.as_ref().map_or(
                    "OR drag and drop your source file here (.mp4, .aedat4, .dat)",
                    |p| p.to_str().unwrap(),
                ),
            );
        });

        ui.horizontal(|ui| {
            if ui.button("Open DVS socket").clicked() {
                if let Some(path) = rfd::FileDialog::new()
                    .set_directory("/tmp")
                    .add_filter("DVS/DAVIS video", &["sock"])
                    .pick_file()
                {
                    self.transcoder_state.core_params.input_path_buf_0 = Some(path.clone());
                }
            }
            if ui.button("Open APS socket").clicked() {
                if let Some(path) = rfd::FileDialog::new()
                    .set_directory("/tmp")
                    .add_filter("DVS/DAVIS video", &["sock"])
                    .pick_file()
                {
                    self.transcoder_state.core_params.input_path_buf_1 = Some(path.clone());
                }
            }
            // if ui.button("Go!").clicked()
            //     && self.transcoder_state.core_params.input_path_buf_0.is_some()
            //     && self.transcoder_state.core_params.input_path_buf_1.is_some()
            // {
            //     replace_adder_transcoder(
            //         self,
            //         self.ui_info_state.input_path_0.clone(),
            //         self.ui_info_state.input_path_1.clone(),
            //         self.ui_info_state.output_path.clone(),
            //         0,
            //     );
            // }
        });
        // ui.label(self.ui_info_state.source_name.clone());

        ui.horizontal(|ui| {
            if ui.button("Save file").clicked() {
                // Check if 'empty' compression is selected, warn user if so
                if self.transcoder_state.core_params.encoder_type == EncoderType::Empty {
                    self.info_ui_state.error_string =
                        Some("Empty compression selected, no output will be written".to_string());
                } else if let Some(mut path) = rfd::FileDialog::new()
                    .add_filter("adder video", &["adder"])
                    .save_file()
                {
                    if !path.ends_with(".adder") {
                        path = path.with_extension("adder");
                    };
                    self.transcoder_state.core_params.output_path = Some(path.clone());
                    dbg!("saving selected");
                    self.info_ui_state.error_string = None;
                }
            }

            let label_opt = &self.transcoder_state.core_params.output_path;

            ui.colored_label(
                if label_opt.is_some() {
                    egui::Color32::GREEN
                } else {
                    ui.style().visuals.text_color()
                },
                label_opt
                    .as_ref()
                    .map_or("No output selected yet", |p| p.to_str().unwrap()),
            );
        });

        Plot::new("quality_plot")
            .height(100.0)
            .allow_drag(true)
            .auto_bounds(Vec2b { x: true, y: true })
            .legend(Legend::default().position(LeftTop))
            .show(ui, |plot_ui| {
                let metrics = vec![
                    (&self.info_ui_state.plot_points_psnr_y, "PSNR dB"),
                    (&self.info_ui_state.plot_points_mse_y, "MSE"),
                    (&self.info_ui_state.plot_points_ssim_y, "SSIM"),
                ];

                for (line, label) in metrics {
                    if line.points.iter().last().unwrap().is_some() {
                        plot_ui.line(line.get_plotline(label, true));
                    }
                }
            });

        Plot::new("bitrate_plot")
            .height(100.0)
            .allow_drag(true)
            .auto_bounds(Vec2b { x: true, y: true })
            .legend(Legend::default().position(LeftTop))
            .show(ui, |plot_ui| {
                let metrics = vec![
                    (
                        &self.info_ui_state.plot_points_raw_adder_bitrate_y,
                        "log Raw ADDER MB/s",
                    ),
                    (
                        &self.info_ui_state.plot_points_raw_source_bitrate_y,
                        "log Raw source MB/s",
                    ),
                ];

                for (line, label) in metrics {
                    if line.points.iter().last().unwrap().is_some() {
                        plot_ui.line(line.get_plotline(label, false));
                    }
                }
            });

        ui.label(format!(
            "{:.2} transcoded FPS\t\
                {:.2} events per source sec\t\
                {:.2} events PPC per source sec\t\
                {:.0} events total\t\
                {:.0} events PPC total",
            self.info_ui_state.transcoded_fps,
            self.info_ui_state.events_per_sec,
            self.info_ui_state.events_ppc_per_sec,
            self.info_ui_state.total_events,
            self.info_ui_state.events_ppc_total
        ));

        let mut avail_size = ui.available_size();
        if self.transcoder_state.adaptive_params.show_original {
            avail_size.x /= 2.0;
        }
        // let images = images.lock().unwrap();

        let size = match (
            self.adder_image_handle.size()[0] as f32,
            self.adder_image_handle.size()[1] as f32,
        ) {
            (a, b) if a / b > avail_size.x / avail_size.y => {
                /*
                The available space has a taller aspect ratio than the video
                Fill the available horizontal space.
                 */
                egui::Vec2 {
                    x: avail_size.x,
                    y: (avail_size.x / a) * b,
                }
            }
            (a, b) => {
                /*
                The available space has a shorter aspect ratio than the video
                Fill the available vertical space.
                 */
                egui::Vec2 {
                    x: (avail_size.y / b) * a,
                    y: avail_size.y,
                }
            }
        };

        ui.horizontal(|ui| {
            if self.transcoder_state.adaptive_params.show_original {
                let input_image = egui::Image::new(egui::load::SizedTexture::new(
                    self.input_image_handle.id(),
                    size,
                ));
                ui.add(input_image);
            }

            let image = egui::Image::new(egui::load::SizedTexture::new(
                self.adder_image_handle.id(),
                size,
            ));
            ui.add(image);
        });
    }

    fn side_panel_grid_contents(&mut self, ui: &mut egui::Ui) {
        let core_params = &mut self.transcoder_state.core_params;
        let adaptive_params = &mut self.transcoder_state.adaptive_params;
        let info_params = &mut self.transcoder_state.info_params;

        let mut slider_button_down = false;

        #[allow(dead_code, unused_mut)]
        let mut enabled = true;
        #[cfg(feature = "open-cv")]
        {
            // enabled = _transcoder.davis_source.is_none();
        }
        ui.add_enabled(enabled, egui::Label::new("Δt_ref:"));
        slider_button_down |= slider_pm(
            enabled,
            false,
            ui,
            &mut core_params.delta_t_ref,
            1..=255,
            vec![],
            10,
        );
        ui.end_row();

        ui.label("Quality parameters:");
        ui.add_enabled(
            true,
            egui::Checkbox::new(&mut adaptive_params.auto_quality, "Auto mode?"),
        );
        // ui.toggle_value(&mut ui_state.auto_quality, "Auto mode?");
        ui.end_row();

        ui.label("CRF quality:");
        slider_button_down |= slider_pm(
            adaptive_params.auto_quality,
            false,
            ui,
            &mut adaptive_params.crf_number,
            0..=CRF.len() as u8 - 1,
            vec![],
            1,
        );
        if adaptive_params.auto_quality
            && adaptive_params.crf_number
                != adaptive_params
                    .encoder_options
                    .crf
                    .get_quality()
                    .unwrap_or(DEFAULT_CRF_QUALITY)
        {
            adaptive_params
                .encoder_options
                .crf
                .update_quality(adaptive_params.crf_number);
        }
        ui.end_row();

        ui.label("Δt_max multiplier:");
        slider_button_down |= slider_pm(
            !adaptive_params.auto_quality,
            false,
            ui,
            &mut core_params.delta_t_max_mult,
            1..=900,
            vec![],
            1,
        );
        ui.end_row();

        ui.label("ADU interval:");
        slider_button_down |= slider_pm(
            true,
            false,
            ui,
            &mut core_params.adu_interval,
            1..=900,
            vec![],
            1,
        );
        ui.end_row();

        let parameters = adaptive_params.encoder_options.crf.get_parameters_mut();
        ui.label("Threshold baseline:");
        slider_button_down |= slider_pm(
            !adaptive_params.auto_quality,
            false,
            ui,
            &mut parameters.c_thresh_baseline,
            0..=255,
            vec![],
            1,
        );
        ui.end_row();

        ui.label("Threshold max:");
        slider_button_down |= slider_pm(
            !adaptive_params.auto_quality,
            false,
            ui,
            &mut parameters.c_thresh_max,
            0..=255,
            vec![],
            1,
        );
        ui.end_row();

        ui.label("Threshold velocity:");
        slider_button_down |= slider_pm(
            !adaptive_params.auto_quality,
            false,
            ui,
            &mut parameters.c_increase_velocity,
            1..=30,
            vec![],
            1,
        );
        ui.end_row();

        ui.label("Feature radius:");
        slider_button_down |= slider_pm(
            !adaptive_params.auto_quality,
            false,
            ui,
            &mut parameters.feature_c_radius,
            0..=100,
            vec![],
            1,
        );
        ui.end_row();

        // ui.label("Thread count:");
        // slider_pm(
        //     true,
        //     false,
        //     ui,
        //     &mut params.thread_count,
        //     &mut params.thread_count_slider,
        //     1..=(current_num_threads() - 1).max(4),
        //     vec![],
        //     1,
        // );
        // ui.end_row();
        //
        ui.label("Video scale:");
        slider_button_down |= slider_pm(
            enabled,
            false,
            ui,
            &mut core_params.scale,
            0.001..=1.0,
            vec![0.25, 0.5, 0.75],
            0.1,
        );
        ui.end_row();
        ui.label("Channels:");
        ui.add_enabled(
            enabled,
            egui::Checkbox::new(&mut core_params.color, "Color?"),
        );
        ui.end_row();

        ui.label("Integration mode:");
        ui.horizontal(|ui| {
            ui.radio_value(
                &mut core_params.integration_mode_radio_state,
                PixelMultiMode::Normal,
                "Normal",
            );
            ui.radio_value(
                &mut core_params.integration_mode_radio_state,
                PixelMultiMode::Collapse,
                "Collapse",
            );
        });
        ui.end_row();

        ui.label("View mode:");
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                ui.radio_value(
                    &mut adaptive_params.view_mode_radio_state,
                    FramedViewMode::Intensity,
                    "Intensity",
                );
                ui.radio_value(
                    &mut adaptive_params.view_mode_radio_state,
                    FramedViewMode::D,
                    "D",
                );
                ui.radio_value(
                    &mut adaptive_params.view_mode_radio_state,
                    FramedViewMode::DeltaT,
                    "Δt",
                );
                ui.radio_value(
                    &mut adaptive_params.view_mode_radio_state,
                    FramedViewMode::SAE,
                    "SAE",
                );
            });
            ui.add_enabled(
                enabled,
                egui::Checkbox::new(&mut adaptive_params.show_original, "Show original?"),
            );
        });
        ui.end_row();

        ui.label("Time mode:");
        ui.add_enabled_ui(true, |ui| {
            ui.horizontal(|ui| {
                ui.radio_value(
                    &mut core_params.time_mode,
                    TimeMode::DeltaT,
                    "Δt (time change)",
                );
                ui.radio_value(
                    &mut core_params.time_mode,
                    TimeMode::AbsoluteT,
                    "t (absolute time)",
                );
            });
        });
        ui.end_row();

        ui.label("Compression mode:");
        let current_encoder_type = core_params.encoder_type;
        ui.add_enabled_ui(true, |ui| {
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    ui.radio_value(
                        &mut core_params.encoder_type,
                        EncoderType::Empty,
                        "Empty (don't write)",
                    );
                    ui.radio_value(&mut core_params.encoder_type, EncoderType::Raw, "Raw");
                });
                ui.horizontal(|ui| {
                    ui.radio_value(
                        &mut core_params.encoder_type,
                        EncoderType::Compressed,
                        "Compressed",
                    );
                });
            });
        });
        if current_encoder_type == EncoderType::Empty
            && current_encoder_type != core_params.encoder_type
        {
            self.info_ui_state.error_string = None;
        }
        ui.end_row();
        //
        #[cfg(feature = "open-cv")]
        {
            ui.label("DAVIS mode:");
            ui.add_enabled_ui(enabled, |ui| {
                ui.horizontal(|ui| {
                    ui.radio_value(
                        &mut core_params.davis_mode_radio_state,
                        TranscoderMode::Framed,
                        "Framed recon",
                    );
                    ui.radio_value(
                        &mut core_params.davis_mode_radio_state,
                        TranscoderMode::RawDavis,
                        "Raw DAVIS",
                    );
                    ui.radio_value(
                        &mut core_params.davis_mode_radio_state,
                        TranscoderMode::RawDvs,
                        "Raw DVS",
                    );
                });
            });
            ui.end_row();

            ui.label("DAVIS deblurred FPS:");

            slider_button_down |= slider_pm(
                enabled,
                true,
                ui,
                &mut core_params.davis_output_fps,
                30.0..=1000000.0,
                vec![
                    50.0, 100.0, 250.0, 500.0, 1_000.0, 2_500.0, 5_000.0, 7_500.0, 10_000.0,
                    1000000.0,
                ],
                50.0,
            );
            ui.end_row();

            let enable_optimize =
                enabled && core_params.davis_mode_radio_state != TranscoderMode::RawDvs;
            ui.label("Optimize:");
            ui.add_enabled(
                enable_optimize,
                egui::Checkbox::new(&mut adaptive_params.optimize_c, "Optimize θ?"),
            );
            ui.end_row();

            ui.label("Optimize frequency:");
            slider_button_down |= slider_pm(
                enable_optimize,
                true,
                ui,
                &mut adaptive_params.optimize_c_frequency,
                1..=250,
                vec![10, 25, 50, 100],
                1,
            );
            ui.end_row();
        }

        let enable_encoder_options = core_params.encoder_type != EncoderType::Empty;

        ui.label("Event output order:");
        ui.add_enabled_ui(enable_encoder_options, |ui| {
            ui.horizontal(|ui| {
                ui.radio_value(
                    &mut adaptive_params.encoder_options.event_order,
                    EventOrder::Unchanged,
                    "Unchanged",
                );
                ui.radio_value(
                    &mut adaptive_params.encoder_options.event_order,
                    EventOrder::Interleaved,
                    "Interleaved",
                );
            });
        });
        ui.end_row();

        ui.label("Bandwidth limiting:");
        ui.add_enabled_ui(true, |ui| {
            ui.horizontal(|ui| {
                ui.radio_value(
                    &mut adaptive_params.encoder_options.event_drop,
                    EventDrop::None,
                    "None",
                );
                if let EventDrop::Manual {
                    target_event_rate,
                    alpha,
                } = adaptive_params.encoder_options.event_drop
                {
                    ui.radio_value(
                        &mut adaptive_params.encoder_options.event_drop,
                        EventDrop::Manual {
                            target_event_rate,
                            alpha,
                        },
                        "Manual",
                    );
                } else {
                    ui.radio_value(
                        &mut adaptive_params.encoder_options.event_drop,
                        EventDrop::Manual {
                            target_event_rate: Default::default(),
                            alpha: Default::default(),
                        },
                        "Manual",
                    );
                }
            });
        });
        ui.end_row();

        if let EventDrop::Manual {
            target_event_rate,
            alpha,
        } = &mut adaptive_params.encoder_options.event_drop
        {
            ui.label("Bandwidth limiting rate:");
            slider_button_down |= slider_pm(
                true,
                true,
                ui,
                target_event_rate,
                1_000.0..=100_000_000.0,
                vec![
                    1_000_000.0,
                    2_500_000.0,
                    5_000_000.0,
                    7_500_000.0,
                    10_000_000.0,
                ],
                50_000.0,
            );
            ui.end_row();

            ui.label("Bandwidth limiting alpha:");

            slider_button_down |= slider_pm(
                true,
                false,
                ui,
                alpha,
                0.0..=1.0,
                vec![0.5, 0.8, 0.9, 0.999, 0.99999, 1.0],
                0.001,
            );
            ui.end_row();
        }

        ui.label("Processing:");
        ui.vertical(|ui| {
            ui.add_enabled(
                true,
                egui::Checkbox::new(&mut adaptive_params.detect_features, "Detect features"),
            );

            ui.add_enabled_ui(adaptive_params.detect_features, |ui| {
                ui.horizontal(|ui| {
                    ui.radio_value(
                        &mut adaptive_params.show_features,
                        ShowFeatureMode::Off,
                        "Don't show",
                    );
                    ui.radio_value(
                        &mut adaptive_params.show_features,
                        ShowFeatureMode::Instant,
                        "Show instant",
                    );
                    ui.radio_value(
                        &mut adaptive_params.show_features,
                        ShowFeatureMode::Hold,
                        "Show & hold",
                    );
                });
                ui.checkbox(
                    &mut adaptive_params.feature_rate_adjustment,
                    "Adjust sensitivities",
                );
                ui.checkbox(&mut adaptive_params.feature_cluster, "Cluster features");
            });
        });
        ui.end_row();

        ui.label("Metrics:");
        ui.vertical(|ui| {
            ui.add_enabled(
                enabled,
                egui::Checkbox::new(&mut info_params.metric_mse, "MSE"),
            );
            ui.add_enabled(
                enabled,
                egui::Checkbox::new(&mut info_params.metric_psnr, "PSNR"),
            );
            ui.add_enabled(
                enabled,
                egui::Checkbox::new(&mut info_params.metric_ssim, "SSIM (Warning: slow!)"),
            );
        });
        ui.end_row();

        self.slider_button_down = slider_button_down;
    }

    //

    //
    //
    //
    //         ui.label(format!(
    //             "{:.2} transcoded FPS\t\
    //             {:.2} events per source sec\t\
    //             {:.2} events PPC per source sec\t\
    //             {:.0} events total\t\
    //             {:.0} events PPC total",
    //             1. / time.delta_seconds(),
    //             self.ui_info_state.events_per_sec,
    //             self.ui_info_state.events_ppc_per_sec,
    //             self.ui_info_state.events_total,
    //             self.ui_info_state.events_ppc_total
    //         ));
    //
    //         if let Some(latency) = self.ui_info_state.davis_latency {
    //             ui.label(format!("DAVIS/DVS latency: {:} ms", latency));
    //         }
    //
    //         self.ui_info_state
    //             .plot_points_eventrate_y
    //             .update(Some(self.ui_info_state.events_ppc_per_sec));
    //
    //         if self.ui_info_state.event_size == 0 {
    //             self.ui_info_state.event_size = if self.ui_info_state.plane.c() == 1 {
    //                 9
    //             } else {
    //                 11
    //             };
    //         }
    //         let bitrate = self.ui_info_state.events_ppc_per_sec
    //             * self.ui_info_state.event_size as f64
    //             * self.ui_info_state.plane.volume() as f64
    //             / 1024.0
    //             / 1024.0; // transcoded raw in megabytes per sec
    //         if self.ui_info_state.plane.volume() > 1 {
    //             self.ui_info_state
    //                 .plot_points_raw_adder_bitrate_y
    //                 .update(Some(bitrate));
    //         } else {
    //             self.ui_info_state
    //                 .plot_points_raw_adder_bitrate_y
    //                 .update(None);
    //         }
    //
    //         self.ui_info_state
    //             .plot_points_latency_y
    //             .update(self.ui_info_state.davis_latency);
    //
    //         // let line_eventrate = self
    //         //     .ui_info_state
    //         //     .plot_points_eventrate_y
    //         //     .get_plotline("Events PPC per sec");
    //
    //         Plot::new("my_plot")
    //             .height(100.0)
    //             .allow_drag(true)
    //             .auto_bounds_y()
    //             .legend(Legend::default().position(LeftTop))
    //             .show(ui, |plot_ui| {
    //                 let metrics = vec![
    //                     (&self.ui_info_state.plot_points_psnr_y, "PSNR dB"),
    //                     (&self.ui_info_state.plot_points_mse_y, "MSE"),
    //                     (&self.ui_info_state.plot_points_ssim_y, "SSIM"),
    //                 ];
    //
    //                 for (line, label) in metrics {
    //                     if line.points.iter().last().unwrap().is_some() {
    //                         plot_ui.line(line.get_plotline(label, false));
    //                     }
    //                 }
    //             });
    //         Plot::new("bitrate_plot")
    //             .height(100.0)
    //             .allow_drag(true)
    //             .auto_bounds_y()
    //             .legend(Legend::default().position(LeftTop))
    //             .show(ui, |plot_ui| {
    //                 let metrics = vec![
    //                     (
    //                         &self.ui_info_state.plot_points_raw_adder_bitrate_y,
    //                         "log10(Raw ADΔER MB/s)",
    //                     ),
    //                     (
    //                         &self.ui_info_state.plot_points_raw_source_bitrate_y,
    //                         "log10(Raw source MB/s)",
    //                     ),
    //                     (&self.ui_info_state.plot_points_latency_y, "Latency"),
    //                 ];
    //
    //                 for (line, label) in metrics {
    //                     if line.points.iter().last().unwrap().is_some() {
    //                         plot_ui.line(line.get_plotline(label, true));
    //                     }
    //                 }
    //             });
    //     }
    //
    //     pub fn update_adder_params(&mut self, _: Res<Images>, mut images: ResMut<Assets<Image>>) {
    //         // TODO: do conditionals on the sliders themselves
    //
    //         let source: &mut dyn Source<BufWriter<File>> = {
    //             match &mut self.transcoder.framed_source {
    //                 None => {
    //                     match &mut self.transcoder.prophesee_source {
    //                         None => {
    //                             #[cfg(feature = "open-cv")]
    //                             match &mut self.transcoder.davis_source {
    //                                 None => {
    //                                     return;
    //                                 }
    //
    //                                 Some(source) => {
    //                                     if source.mode != self.ui_state.davis_mode_radio_state
    //                                         || source.get_reconstructor().as_ref().unwrap().output_fps
    //                                             != self.ui_state.davis_output_fps
    //                                         || ((source.get_video_ref().get_time_mode()
    //                                             != self.ui_state.time_mode
    //                                             || source.get_video_ref().encoder_type
    //                                                 != self.ui_state.encoder_type
    //                                             || source
    //                                                 .get_video_ref()
    //                                                 .get_encoder_options()
    //                                                 .event_drop
    //                                                 != self.ui_state.encoder_options.event_drop
    //                                             || source
    //                                                 .get_video_ref()
    //                                                 .get_encoder_options()
    //                                                 .event_order
    //                                                 != self.ui_state.encoder_options.event_order
    //                                             || source
    //                                                 .get_video_ref()
    //                                                 .state
    //                                                 .params
    //                                                 .pixel_multi_mode
    //                                                 != self.ui_state.integration_mode_radio_state)
    //                                             && self.ui_info_state.output_path.is_some())
    //                                     {
    //                                         if self.ui_state.davis_mode_radio_state == RawDvs {
    //                                             // self.ui_state.davis_output_fps = 1000000.0;
    //                                             // self.ui_state.davis_output_fps_slider = 1000000.0;
    //                                             self.ui_state.optimize_c = false;
    //                                         }
    //                                         replace_adder_transcoder(
    //                                             self,
    //                                             self.ui_info_state.input_path_0.clone(),
    //                                             self.ui_info_state.input_path_1.clone(),
    //                                             self.ui_info_state.output_path.clone(),
    //                                             0,
    //                                         );
    //                                         images.clear();
    //                                         return;
    //                                     }
    //                                     let tmp = source.get_reconstructor_mut().as_mut().unwrap();
    //                                     tmp.set_optimize_c(
    //                                         self.ui_state.optimize_c,
    //                                         self.ui_state.optimize_c_frequency,
    //                                     );
    //                                     source
    //                                 }
    //                             }
    //                             #[cfg(not(feature = "open-cv"))]
    //                             return;
    //                         }
    //                         Some(source) => {
    //                             if source.get_video_ref().get_ref_time()
    //                                 != self.ui_state.delta_t_ref as u32
    //                                 || ((source.get_video_ref().get_time_mode()
    //                                     != self.ui_state.time_mode
    //                                     || source.get_video_ref().encoder_type
    //                                         != self.ui_state.encoder_type
    //                                     || source.get_video_ref().get_encoder_options().event_drop
    //                                         != self.ui_state.encoder_options.event_drop
    //                                     || source.get_video_ref().get_encoder_options().event_order
    //                                         != self.ui_state.encoder_options.event_order)
    //                                     && self.ui_info_state.output_path.is_some())
    //                             {
    //                                 images.clear();
    //                                 replace_adder_transcoder(
    //                                     self,
    //                                     self.ui_info_state.input_path_0.clone(),
    //                                     self.ui_info_state.input_path_1.clone(),
    //                                     self.ui_info_state.output_path.clone(),
    //                                     0,
    //                                 );
    //                                 return;
    //                             }
    //
    //                             source
    //                         }
    //                     }
    //                 }
    //                 Some(source) => {
    //                     if source.scale != self.ui_state.scale
    //                         || source.get_ref_time() != self.ui_state.delta_t_ref as u32
    //                         || ((source.get_video_ref().get_time_mode() != self.ui_state.time_mode
    //                             || source.get_video_ref().encoder_type != self.ui_state.encoder_type
    //                             || source.get_video_ref().get_encoder_options().event_drop
    //                                 != self.ui_state.encoder_options.event_drop
    //                             || source.get_video_ref().get_encoder_options().event_order
    //                                 != self.ui_state.encoder_options.event_order)
    //                             && self.ui_info_state.output_path.is_some())
    //                         || match source.get_video_ref().state.plane.c() {
    //                             1 => {
    //                                 // True if the transcoder is gray, but the user wants color
    //                                 self.ui_state.color
    //                             }
    //                             _ => {
    //                                 // True if the transcoder is color, but the user wants gray
    //                                 !self.ui_state.color
    //                             }
    //                         }
    //                     {
    //                         let current_frame =
    //                             source.get_video_ref().state.in_interval_count + source.frame_idx_start;
    //                         images.clear();
    //                         replace_adder_transcoder(
    //                             self,
    //                             self.ui_info_state.input_path_0.clone(),
    //                             self.ui_info_state.input_path_1.clone(),
    //                             self.ui_info_state.output_path.clone(),
    //                             current_frame,
    //                         );
    //                         return;
    //                     }
    //                     source
    //                 }
    //             }
    //         };
    //
    //         let binding = source.get_video_ref().get_encoder_options();
    //         let _parameters = binding.crf.get_parameters();
    //
    //         // TODO: Refactor all this garbage code
    //         if self.ui_state.auto_quality
    //             && (!self.ui_state.auto_quality_mirror
    //                 || self.ui_state.encoder_options.crf.get_quality()
    //                     != source
    //                         .get_video_ref()
    //                         .get_encoder_options()
    //                         .crf
    //                         .get_quality())
    //         {
    //             self.ui_state.auto_quality_mirror = true;
    //             source.crf(
    //                 self.ui_state
    //                     .encoder_options
    //                     .crf
    //                     .get_quality()
    //                     .unwrap_or(DEFAULT_CRF_QUALITY),
    //             );
    //
    //             let video = source.get_video_ref();
    //
    //             let binding = video.get_encoder_options();
    //             let parameters = binding.crf.get_parameters();
    //
    //             self.ui_state.encoder_options = binding;
    //             // Update ui state to match
    //             self.ui_state.crf_slider = binding.crf.get_quality().unwrap_or(DEFAULT_CRF_QUALITY);
    //             self.ui_state.adder_tresh_baseline_slider = parameters.c_thresh_baseline;
    //             self.ui_state.adder_tresh_max_slider = parameters.c_thresh_max;
    //             self.ui_state.delta_t_max_mult =
    //                 video.state.params.delta_t_max / video.state.params.ref_time;
    //             self.ui_state.delta_t_max_mult_slider = self.ui_state.delta_t_max_mult;
    //             self.ui_state.adder_tresh_velocity_slider = parameters.c_increase_velocity;
    //             self.ui_state.feature_radius_slider = parameters.feature_c_radius;
    //         } else if !self.ui_state.auto_quality
    //             && (self.ui_state.delta_t_max_mult
    //                 != source.get_video_ref().state.params.delta_t_max
    //                     / source.get_video_ref().state.params.ref_time
    //                 || self.ui_state.encoder_options.crf.get_parameters()
    //                     != source
    //                         .get_video_ref()
    //                         .get_encoder_options()
    //                         .crf
    //                         .get_parameters())
    //         {
    //             let video = source.get_video_mut();
    //             let parameters = self.ui_state.encoder_options.crf.get_parameters();
    //             video.update_quality_manual(
    //                 parameters.c_thresh_baseline,
    //                 parameters.c_thresh_max,
    //                 self.ui_state.delta_t_max_mult,
    //                 parameters.c_increase_velocity,
    //                 parameters.feature_c_radius as f32,
    //             )
    //         }
    //
    //         if !self.ui_state.auto_quality {
    //             self.ui_state.auto_quality_mirror = false;
    //         }
    //         let video = source.get_video_mut();
    //
    //         if video.state.params.pixel_multi_mode != self.ui_state.integration_mode_radio_state {
    //             video.state.params.pixel_multi_mode = self.ui_state.integration_mode_radio_state;
    //         }
    //
    //         self.ui_info_state.event_size = video.get_event_size();
    //         self.ui_info_state.plane = video.state.plane;
    //
    //         video.instantaneous_view_mode = self.ui_state.view_mode_radio_state;
    //         video.update_detect_features(
    //             self.ui_state.detect_features,
    //             self.ui_state.show_features,
    //             self.ui_state.feature_rate_adjustment,
    //             self.ui_state.feature_cluster,
    //         );
    //     }
    //
    //     pub fn consume_source(
    //         &mut self,
    //         mut images: ResMut<Assets<Image>>,
    //         mut handles: ResMut<Images>,
    //     ) -> Result<(), Box<dyn Error>> {
    //         let pool = rayon::ThreadPoolBuilder::new()
    //             .num_threads(self.ui_state.thread_count)
    //             .build()?;
    //
    //         let ui_info_state = &mut self.ui_info_state;
    //         ui_info_state.events_per_sec = 0.;
    //
    //         // TODO: The below code is absolutely horrible.
    //         let source: &mut dyn Source<BufWriter<File>> = {
    //             match &mut self.transcoder.framed_source {
    //                 None => match &mut self.transcoder.prophesee_source {
    //                     None => {
    //                         #[cfg(feature = "open-cv")]
    //                         match &mut self.transcoder.davis_source {
    //                             None => {
    //                                 return Ok(());
    //                             }
    //                             Some(source) => {
    //                                 ui_info_state.davis_latency = Some(source.get_latency() as f64);
    //                                 source
    //                             }
    //                         }
    //                         #[cfg(not(feature = "open-cv"))]
    //                         return Ok(());
    //                     }
    //                     Some(source) => source,
    //                 },
    //                 Some(source) => source,
    //             }
    //         };
    //
    //         match source.consume(&pool) {
    //             Ok(events_vec_vec) => {
    //                 for events_vec in events_vec_vec {
    //                     ui_info_state.events_total += events_vec.len() as u64;
    //                     ui_info_state.events_per_sec += events_vec.len() as f64;
    //                 }
    //                 ui_info_state.events_ppc_total = ui_info_state.events_total as f64
    //                     / (source.get_video_ref().state.plane.volume() as f64);
    //                 let source_fps = source.get_video_ref().get_tps() as f64
    //                     / source.get_video_ref().get_ref_time() as f64;
    //                 ui_info_state.events_per_sec *= source_fps;
    //                 ui_info_state.events_ppc_per_sec = ui_info_state.events_per_sec
    //                     / (source.get_video_ref().state.plane.volume() as f64);
    //             }
    //             Err(SourceError::Open) => {}
    //             Err(e) => {
    //                 eprintln!("Error: {:?}", e);
    //                 source.get_video_mut().end_write_stream()?;
    //                 self.ui_info_state.output_path = None;
    //                 self.ui_info_state.output_name = Default::default();
    //
    //                 // Start video over from the beginning
    //                 replace_adder_transcoder(
    //                     self,
    //                     self.ui_info_state.input_path_0.clone(),
    //                     self.ui_info_state.input_path_1.clone(),
    //                     None,
    //                     0,
    //                 );
    //                 return Ok(());
    //             }
    //         };
    //
    //         // Calculate quality metrics on the running intensity frame (not with features drawn on it)
    //         let image_mat = &source.get_video_ref().state.running_intensities;
    //
    //         if let Some(input) = source.get_input() {
    //             #[rustfmt::skip]
    //             let metrics = calculate_quality_metrics(
    //                 input,
    //                 image_mat,
    //                 QualityMetrics {
    //                     mse: if self.ui_state.metric_mse {Some(0.0)} else {None},
    //                     psnr: if self.ui_state.metric_psnr {Some(0.0)} else {None},
    //                     ssim: if self.ui_state.metric_ssim {Some(0.0)} else {None},
    //                 },
    //             );
    //             let metrics = metrics?;
    //             self.ui_info_state.plot_points_psnr_y.update(metrics.psnr);
    //             self.ui_info_state.plot_points_mse_y.update(metrics.mse);
    //             self.ui_info_state.plot_points_ssim_y.update(metrics.ssim);
    //         }
    //
    //         // Display frame
    //         let image_mat = source.get_video_ref().display_frame_features.clone();
    //
    //         let color = image_mat.shape()[2] == 3;
    //
    //         if let Some(image) = images.get_mut(&handles.image_view) {
    //             crate::utils::prep_bevy_image_mut(image_mat, color, image)?;
    //         } else {
    //             // dbg!("else");
    //             let image_bevy = prep_bevy_image(
    //                 image_mat,
    //                 color,
    //                 source.get_video_ref().state.plane.w(),
    //                 source.get_video_ref().state.plane.h(),
    //             )?;
    //             self.transcoder.live_image = image_bevy;
    //             let handle = images.add(self.transcoder.live_image.clone());
    //             handles.image_view = handle;
    //         }
    //
    //         // Repeat for the input view
    //         if self.ui_state.show_original && source.get_input().is_some() {
    //             let image_mat = source.get_input().unwrap();
    //             let image_mat = image_mat.clone();
    //             let color = image_mat.shape()[2] == 3;
    //
    //             if let Some(image) = images.get_mut(&handles.input_view) {
    //                 crate::utils::prep_bevy_image_mut(image_mat, color, image)?;
    //             } else {
    //                 let image_bevy = prep_bevy_image(
    //                     image_mat,
    //                     color,
    //                     source.get_video_ref().state.plane.w(),
    //                     source.get_video_ref().state.plane.h(),
    //                 )?;
    //                 let handle = images.add(image_bevy);
    //                 handles.input_view = handle;
    //             }
    //         }
    //         if !self.ui_state.show_original {
    //             handles.input_view = Default::default();
    //         }
    //
    //         let raw_source_bitrate = source.get_running_input_bitrate() / 8.0 / 1024.0 / 1024.0; // source in megabytes per sec
    //         self.ui_info_state
    //             .plot_points_raw_source_bitrate_y
    //             .update(Some(raw_source_bitrate));
    //
    //         Ok(())
    //     }
}
//
