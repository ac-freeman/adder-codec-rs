use crate::transcoder::adder::AdderTranscoder;
use crate::transcoder::{AdaptiveParams, CoreParams, EventRateMsg, InfoParams, InfoUiState};
use crate::utils::slider_pm;
use crate::TabState;
use adder_codec_rs::adder_codec_core;
use adder_codec_rs::adder_codec_core::codec::rate_controller::{CRF, DEFAULT_CRF_QUALITY};
use adder_codec_rs::adder_codec_core::codec::{EncoderType, EventDrop, EventOrder};
use adder_codec_rs::adder_codec_core::{Coord, PixelMultiMode, PlaneSize, TimeMode};
#[cfg(feature = "open-cv")]
use adder_codec_rs::transcoder::source::davis::TranscoderMode;
use adder_codec_rs::transcoder::source::video::FramedViewMode;
use adder_codec_rs::utils::cv::QualityMetrics;
use adder_codec_rs::utils::viz::ShowFeatureMode;
use eframe::epaint::ColorImage;
use egui::Vec2b;
use egui_plot::Corner::LeftTop;
use egui_plot::{Legend, Plot};
use tokio::sync::mpsc;
use tokio::sync::mpsc::Sender;

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

#[derive(Default, Debug, Clone, PartialEq, Copy)]
pub struct Roi {
    pub start: Option<egui::Pos2>,
    pub end: Option<egui::Pos2>,
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
    pub roi: Roi,
    pub is_drawing_roi: bool,
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
            roi: Default::default(),
            is_drawing_roi: false,
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

        let mut style = (*ctx.style()).clone(); // Clone the current style
        style.interaction.tooltip_delay = 0.05;
        ctx.set_style(style);

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
        while let Ok(message) = self.msg_rx.try_recv() {
            match message {
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
                    self.roi = Roi::default();
                    if self.transcoder_state.adaptive_params.auto_quality {
                        self.transcoder_state
                            .adaptive_params
                            .encoder_options
                            .crf
                            .update_quality(self.transcoder_state.adaptive_params.crf_number);
                    }
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
            let response = ui.add(image);

            // Get the bounding rectangle of the image view
            let image_rect = response.rect;

            // If the rect coordinates have NAN values, skip the following
            if image_rect.min.x.is_nan() || image_rect.min.y.is_nan() {
                return;
            }

            // Handle mouse input for ROI
            ui.ctx().input(|i| {
                if let Some(pos) = i.pointer.interact_pos() {
                    if i.pointer.any_pressed() && image_rect.contains(pos) {
                        if !self.is_drawing_roi {
                            self.roi.start = Some(pos);
                            self.is_drawing_roi = true;

                            // self.roi.end = Some(pos);
                            // eprintln!("Ending ROI at: {:?}", pos);
                        }
                    } else if i.pointer.any_down() && self.is_drawing_roi {
                        // Move the endpoint for live visualization of the ROI
                        let clamped_pos = egui::Pos2 {
                            x: pos.x.clamp(image_rect.min.x, image_rect.max.x),
                            y: pos.y.clamp(image_rect.min.y, image_rect.max.y),
                        };
                        self.roi.end = Some(clamped_pos);
                    } else if i.pointer.any_released() && self.is_drawing_roi {
                        let clamped_pos = egui::Pos2 {
                            x: pos.x.clamp(image_rect.min.x, image_rect.max.x),
                            y: pos.y.clamp(image_rect.min.y, image_rect.max.y),
                        };
                        self.roi.end = Some(clamped_pos);
                        self.is_drawing_roi = false;

                        // Send the ROI to the transcoder
                        // Subtract the image_rect min from the ROI coordinates
                        let roi_start = self.roi.start.unwrap_or_default() - image_rect.min;
                        let roi_end = self.roi.end.unwrap_or_default() - image_rect.min;
                        println!("ROI start: {:?}, end: {:?}", roi_start, roi_end);

                        // Undo the coordinate scaling from size to self.adder_image_handle.size()[0]
                        let scale_x = self.adder_image_handle.size()[0] as f32 / size.x;
                        let scale_y = self.adder_image_handle.size()[1] as f32 / size.y;
                        let roi_start = egui::Pos2 {
                            x: (roi_start.x * scale_x).round(),
                            y: (roi_start.y * scale_y).round(),
                        };
                        let roi_end = egui::Pos2 {
                            x: (roi_end.x * scale_x).round(),
                            y: (roi_end.y * scale_y).round(),
                        };
                        println!("Scaled ROI start: {:?}, end: {:?}", roi_start, roi_end);

                        self.transcoder_state.adaptive_params.roi =
                            Some(adder_codec_rs::transcoder::source::video::Roi {
                                start: Coord {
                                    x: roi_start.x as u16,
                                    y: roi_start.y as u16,
                                    c: None,
                                },
                                end: Coord {
                                    x: roi_end.x as u16,
                                    y: roi_end.y as u16,
                                    c: None,
                                },
                            })
                    }
                }
            });

            // Draw the bounding box
            if let (Some(start), Some(end)) = (self.roi.start, self.roi.end) {
                let rect = egui::Rect::from_two_pos(start, end);
                ui.ctx()
                    .layer_painter(egui::LayerId::new(
                        egui::Order::Foreground,
                        egui::Id::new("roi"),
                    ))
                    .rect_stroke(rect, 0.0, egui::Stroke::new(2.0, egui::Color32::RED));
            }
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
        label_with_help_cursor(
            ui,
            "Δt_ref:",
            Some(
                "The number of ticks for a standard length integration (e.g. exposure
             time for a framed video).",
            ),
        );
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
        ui.end_row();

        label_with_help_cursor(
            ui,
            "CRF Quality:",
            Some(
                "Constant Rate Factor is a metaparameter that controls data rate and
            loss by adjusting multiple variables. Setting a high value will produce 
            greater loss but a lower data rate. CRF values 0, 3, 6, & 9 are 
            lossless, high, medium, & low quality, respectively.",
            ),
        );
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
        //add informational hover button

        ui.end_row();

        label_with_help_cursor(
            ui,
            "Δt_max multiplier:",
            Some(
                "The maximum Δt that an event can span before the first update
            is internally fired.",
            ),
        );
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

        label_with_help_cursor(
            ui,
            "ADU interval:",
            Some("The number of Δt_ref intervals spanned by an ADU when compression is enabled."),
        );
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
        label_with_help_cursor(
            ui,
            "Threshold baseline:",
            Some("Default contrast threshold."),
        );
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
        label_with_help_cursor(ui, "Threshold max:", Some("Maximum contrast threshold."));
        slider_button_down |= slider_pm(
            !adaptive_params.auto_quality,
            false,
            ui,
            &mut parameters.c_thresh_max,
            0..=255,
            vec![],
            1,
        );
        ui.add_space(-80.0);
        label_with_help_cursor(
            ui,
            "Threshold?",
            Some(
                "The amount of variation in intensity allowed, affecting length
            of integration until an event queue is fired and pixel is reset.",
            ),
        );

        ui.end_row();

        label_with_help_cursor(
            ui,
            "Threshold velocity",
            Some("The frequency at which pixels' threshold values increase."),
        );
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

        label_with_help_cursor(
            ui,
            "Feature radius:",
            Some(
                "The radius for which to reset the contrast threshold for neighboring pixels when
             a feature is detected (if enabled)",
            ),
        );
        slider_button_down |= slider_pm(
            !adaptive_params.auto_quality,
            false,
            ui,
            &mut parameters.feature_c_radius,
            0..=100,
            vec![],
            1,
        );
        //add informational hover button
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
        label_with_help_cursor(
            ui,
            "Video scale:",
            Some(
                "Spatial resolution, compared to the original video. Input video will be downscaled
            before transcoding. 1.0 = original resolution, 0.5 = half resolution, etc.",
            ),
        );
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
        label_with_help_cursor(ui, "Channels:", Some("Color (if supported) or monochrome?"));
        ui.add_enabled(
            enabled,
            egui::Checkbox::new(&mut core_params.color, "Color?"),
        );
        ui.end_row();
        label_with_help_cursor(
            ui,
            "Integration mode:",
            Some(
                "Normal mode will produce all events, similar to what an integrating event sensor
            would capture. Collapse mode will only prouduce the first and last events at a new
            intensity level, once the contrast threshold is exceeded.",
            ),
        );
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

        label_with_help_cursor(
            ui,
            "View mode",
            Some(
                "The view mode for the video.
            Intensity will show the intensity of the pixels,
            D will show the decimation components of events as they fire,
            DeltaT will show the time since the last event,
            and SAE is the surface of active events.",
            ),
        );
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

        label_with_help_cursor(ui, "Time mode:", None);
        ui.add_enabled_ui(true, |ui| {
            ui.horizontal(|ui| {
                ui.radio_value(
                    &mut core_params.time_mode,
                    TimeMode::DeltaT,
                    "Δt (time change)",
                )
                .on_hover_text(
                    "measures temporal values based 
                on previous data",
                );
                ui.radio_value(
                    &mut core_params.time_mode,
                    TimeMode::AbsoluteT,
                    "t (absolute time)",
                )
                .on_hover_text(
                    "measures temporal values independent 
                of previous data",
                );
            });
        });
        ui.end_row();

        label_with_help_cursor(
            ui,
            "Compression mode:",
            Some(
                "Empty does not write any data to the output file, which can be faster for viz.
            Raw writes uncompressed event tuples.
            Compressed writes compressed events using the bespoke encoder.",
            ),
        );
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
        #[cfg(feature = "open-cv")]
        {
            label_with_help_cursor(
                ui,
                "DAVIS mode:",
                Some("Framed recon performs a framed reconstruction of the events and frames using EDI.
                Raw DAVIS integrates events onto deblurred frames in the event space (much faster).
                Raw DVS simply integrates the DVS events alone.")
            );
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

            label_with_help_cursor(
                ui,
                "DAVIS deblurred FPS:",
                Some(
                    "If DAVIS mode is \"Framed recon\" or \"Raw DAVIS\", this determines the
                effective shutter speed of the deblurred APS frames. For example, if this parameter
                is 100, each deblurred frame will span 10ms.",
                ),
            );

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
            label_with_help_cursor(
                ui,
                "Optimize:",
                Some("Continually optimize the θ contrast threshold for DVS?"),
            );
            ui.add_enabled(
                enable_optimize,
                egui::Checkbox::new(&mut adaptive_params.optimize_c, "Optimize θ?"),
            );
            ui.end_row();

            label_with_help_cursor(
                ui,
                "Optimize frequency:",
                Some("How many input APS frames between each θ optimization (if enabled)"),
            );
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
        label_with_help_cursor(
            ui,
            "Event output order:",
            Some(
                "Unchanged may produce events from different pixels that are not temporally sorted,
            relative to each other. Interleaved will temporally sort the events of all pixels, but
            it will be slightly slower.",
            ),
        );
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

        label_with_help_cursor(
            ui,
            "Bandwidth limiting:",
            Some("The rate is the maximum number of events per second that will be sent to the encoder.\
            The alpha is the decay rate of the bandwidth limiting with an exponential smoothing function.
            A value of 1.0 means that the bandwidth limiting will be instantaneous, while lower
            values will give more weight in the rate estimation to the previous measure rate.")
        );
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
}

fn label_with_help_cursor(ui: &mut egui::Ui, text: &str, hover_text: Option<&str>) {
    let label = ui.label(text);
    if let Some(hover) = hover_text {
        if label.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Help);
        }
        label.on_hover_text(hover);
    }
}
