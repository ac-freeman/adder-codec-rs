use crate::player::adder::AdderPlayer;
use crate::player::{AdaptiveParams, CoreParams};
use crate::transcoder::InfoParams;
use crate::utils::{add_checkbox_row, add_slider_row};
use crate::{slider_pm, TabState, VizUi};
use adder_codec_rs::adder_codec_core::PlaneSize;
use adder_codec_rs::transcoder::source::video::FramedViewMode;
use eframe::epaint::ColorImage;
use egui::Ui;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tokio::sync::mpsc::error::TryRecvError;
use tokio::sync::mpsc::{Receiver, Sender};

#[derive(Debug, Clone)]
pub enum PlayerStateMsg {
    Terminate,
    Set { player_state: PlayerState },
    Loop { player_state: PlayerState },
}

#[derive(Debug, Clone)]
pub enum PlayerInfoMsg {
    Plane((PlaneSize, bool)),
    FrameLength(Duration),
    // EventRateMsg(EventRateMsg),
    // Image(ColorImage),
    Error(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlayerState {
    pub adaptive_params: AdaptiveParams,
    pub core_params: CoreParams,
    // pub info_params: InfoParams,
}

impl Default for PlayerState {
    fn default() -> Self {
        Self {
            adaptive_params: Default::default(),
            core_params: Default::default(),
            // info_params: Default::default(),
        }
    }
}

impl TabState for PlayerState {
    fn reset_params(&mut self) {
        self.adaptive_params = Default::default();
        let input_path_buf_0 = self.core_params.input_path_buf_0.clone();
        self.core_params = Default::default();
        self.core_params.input_path_buf_0 = input_path_buf_0;
    }

    fn reset_video(&mut self) {
        self.core_params.input_path_buf_0 = None;
    }
}

pub struct PlayerUi {
    pub player_state: PlayerState,
    adder_image_handle: egui::TextureHandle,
    last_frame_time: std::time::Instant,
    pub player_state_tx: Sender<PlayerStateMsg>,
    msg_rx: mpsc::Receiver<PlayerInfoMsg>,
    pub image_rx: Receiver<ColorImage>,
    pub last_frame_display_time: Option<Instant>,
    pub frame_length: Duration,
    pub paused: Arc<AtomicBool>,
}

impl PlayerUi {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let (tx, mut rx) = mpsc::channel::<PlayerStateMsg>(5);
        let (msg_tx, mut msg_rx) = mpsc::channel::<PlayerInfoMsg>(30);
        let (image_tx, mut image_rx) = mpsc::channel::<ColorImage>(500);

        let mut player_ui = PlayerUi {
            player_state: PlayerState::default(),
            adder_image_handle: cc.egui_ctx.load_texture(
                "adder_image",
                ColorImage::default(),
                Default::default(),
            ),
            last_frame_time: std::time::Instant::now(),
            player_state_tx: tx,
            msg_rx,
            image_rx,
            last_frame_display_time: None,
            frame_length: Duration::from_secs_f32(1.0 / 30.0),
            paused: Arc::new(false.into()),
        };

        player_ui.spawn_tab_runner(rx, msg_tx, image_tx);
        player_ui
    }

    fn spawn_tab_runner(
        &mut self,
        rx: mpsc::Receiver<PlayerStateMsg>,
        msg_tx: mpsc::Sender<PlayerInfoMsg>,
        image_tx: Sender<ColorImage>,
    ) {
        let adder_image_handle = self.adder_image_handle.clone();
        let rt = tokio::runtime::Runtime::new().expect("Unable to create Runtime");

        let _enter = rt.enter();

        // Execute the runtime in its own thread.
        std::thread::spawn(move || {
            rt.block_on(async {
                let mut transcoder = AdderPlayer::new(rx, msg_tx, image_tx);
                transcoder.run().await;
            })
        });
    }

    pub fn update(&mut self, ctx: &egui::Context) {
        // Store a copy of the params to compare against later
        let old_params = self.player_state.clone();

        // Collect dropped files
        self.handle_file_drop(ctx);

        self.handle_info_messages();

        self.draw_ui(ctx);

        // This should always be the very last thing we do in this function
        if old_params != self.player_state {
            eprintln!("Sending new transcoder state");
            let res = self.player_state_tx.blocking_send(PlayerStateMsg::Set {
                player_state: self.player_state.clone(),
            });
        }
    }

    fn handle_file_drop(&mut self, ctx: &egui::Context) {
        ctx.input(|i| {
            if !i.raw.dropped_files.is_empty() {
                self.player_state.core_params.input_path_buf_0 =
                    i.raw.dropped_files[0].path.clone();
            }
        });
    }

    fn handle_info_messages(&mut self) {
        loop {
            match self.msg_rx.try_recv() {
                Ok(PlayerInfoMsg::Plane((plane, _))) => {
                    // self.player_state.info_params.plane = plane;
                }
                Ok(PlayerInfoMsg::FrameLength(frame_length)) => {
                    eprintln!("Setting new frame length: {:?}", frame_length);
                    self.frame_length = frame_length;
                }
                Ok(PlayerInfoMsg::Error(e)) => {
                    eprintln!("Error: {}", e);
                }
                _ => break,
            }
        }
    }
}

impl VizUi for PlayerUi {
    fn draw_ui(&mut self, ctx: &egui::Context) {
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
                self.player_state.reset_params();
            }
            if ui.add(egui::Button::new("Reset video")).clicked() {
                self.player_state.reset_video();
                // self.transcoder_state_tx
                //     .blocking_send(TranscoderStateMsg::Terminate)
                //     .unwrap();
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

    fn central_panel_ui(&mut self, ui: &mut egui::Ui) {
        let mut avail_size = ui.available_size();

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

        let image = egui::Image::new(egui::load::SizedTexture::new(
            self.adder_image_handle.id(),
            size,
        ));
        ui.add(image);

        let time_since_last_displayed = match self.last_frame_display_time {
            None => self.frame_length,
            Some(a) => a.elapsed(),
        };

        if !self.paused.load(Ordering::Relaxed) && time_since_last_displayed >= self.frame_length {
            // Get the next image
            match self.image_rx.try_recv() {
                Ok(image) => {
                    self.adder_image_handle.set(image, Default::default());
                    self.last_frame_display_time = Some(Instant::now());
                }
                Err(_) => {
                    // If we don't have a new image to display, sleep this thread (buffered pause)
                    // Sleep 1 second
                    if self.last_frame_display_time.is_some() {
                        self.paused.store(true, Ordering::Relaxed);

                        // Spawn a thread to mark the player as unpaused after 3 seconds
                        let paused = self.paused.clone();
                        std::thread::spawn(move || {
                            dbg!("Sleeping 3 seconds...");
                            std::thread::sleep(Duration::from_secs(3));
                            paused.store(false, Ordering::Relaxed);
                        });
                    }
                }
            }
        }
    }

    fn side_panel_grid_contents(&mut self, ui: &mut Ui) {
        let player_state_copy = self.player_state.clone();
        let core_params = &mut self.player_state.core_params;
        let adaptive_params = &mut self.player_state.adaptive_params;
        // let info_params = &mut self.transcoder_state.info_params;

        ui.label("Playback speed:");
        let playback_speed = core_params.playback_speed;
        slider_pm(
            true,
            true,
            ui,
            &mut core_params.playback_speed,
            0.001..=1000.0,
            vec![0.25, 0.5, 1.0, 5.0, 10.0],
            1.0,
        );
        ui.end_row();
        if playback_speed != core_params.playback_speed {
            while self.image_rx.try_recv().is_ok() {} // Drain the image channel
        }

        ui.add_enabled(true, egui::Label::new("Playback controls:"));
        ui.horizontal(|ui| {
            if !self.paused.load(Ordering::Relaxed) {
                if ui.button("⏸").clicked() {
                    self.paused.store(true, Ordering::Relaxed);
                }
            } else if ui.button("▶").clicked() {
                self.paused.store(false, Ordering::Relaxed);
            }
            // TODO: remove this?
            if ui.button("⏹").clicked() {
                self.paused.store(false, Ordering::Relaxed);
                core_params.input_path_buf_0 = None;
            }

            if ui.button("⏮").clicked() {
                self.paused.store(false, Ordering::Relaxed);
                // Send a Loop message
                let res = self.player_state_tx.blocking_send(PlayerStateMsg::Loop {
                    player_state: player_state_copy,
                });
                while self.image_rx.try_recv().is_ok() {} // Drain the image channel
            }
        });
        ui.end_row();

        let mut limit_frame_buffer_bool = adaptive_params.buffer_limit.is_some();
        add_checkbox_row(
            true,
            "Frame buffer",
            "Limit frame buffer?",
            ui,
            &mut limit_frame_buffer_bool,
        );

        if limit_frame_buffer_bool && adaptive_params.buffer_limit.is_none() {
            // If the user just selected to limit the frame buffer, set the default value
            adaptive_params.buffer_limit = Some(100);
        } else if !limit_frame_buffer_bool {
            adaptive_params.buffer_limit = None;
        }

        let mut buffer_limit = adaptive_params.buffer_limit.unwrap_or(100);
        add_slider_row(
            limit_frame_buffer_bool,
            false,
            "Buffer limit:",
            ui,
            &mut buffer_limit,
            0..=1000,
            vec![10, 100, 250, 500, 750],
            10,
        );

        if limit_frame_buffer_bool {
            adaptive_params.buffer_limit = Some(buffer_limit);
        }

        crate::add_radio_row(
            true,
            "View mode:",
            vec![
                ("Intensity", FramedViewMode::Intensity),
                ("D", FramedViewMode::D),
                ("Δt", FramedViewMode::DeltaT),
                ("SAE", FramedViewMode::SAE),
            ],
            ui,
            &mut adaptive_params.view_mode,
        );

        ui.label("Processing:");
        ui.add_enabled(
            true,
            egui::Checkbox::new(&mut adaptive_params.detect_features, "Detect features"),
        );
        ui.end_row();
    }
}
