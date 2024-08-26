use crate::player::adder::AdderPlayer;
use crate::player::{AdaptiveParams, CoreParams};
use crate::transcoder::InfoParams;
use crate::utils::{add_checkbox_row, add_slider_row};
use crate::{slider_pm, TabState, VizUi};
use adder_codec_rs::adder_codec_core::PlaneSize;
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
            dbg!(res);
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
    }
}

// use crossbeam_channel::{bounded, Receiver};
// use std::collections::VecDeque;
// use std::error::Error;
// use std::path::PathBuf;
// use std::time::Duration;
//
// use adder_codec_rs::transcoder::source::video::FramedViewMode;
//
// use crate::player::adder::{AdderPlayer, PlayerStreamArtifact, StreamState};
// use crate::{add_checkbox_row, add_radio_row, add_slider_row, Images};
//
// use crate::utils::PlotY;
// use adder_codec_rs::adder_codec_core::PlaneSize;
// use rayon::current_num_threads;
//
// #[derive(PartialEq)]
// pub struct PlayerUiSliders {
//     playback_speed: f32,
//     thread_count: usize,
// }
//
// impl Default for PlayerUiSliders {
//     fn default() -> Self {
//         Self {
//             playback_speed: 1.0,
//             thread_count: 4,
//         }
//     }
// }
//
// #[derive(PartialEq, Clone)]
// pub enum ReconstructionMethod {
//     Fast,
//     Accurate,
// }
//
// impl Default for ReconstructionMethod {
//     fn default() -> Self {
//         Self::Accurate
//     }
// }
//
// pub struct PlayerUiState {
//     playing: bool,
//     looping: bool,
//     view_mode: FramedViewMode,
//     reconstruction_method: ReconstructionMethod,
//     current_frame: u32,
//     total_frames: u32,
//     current_time: f32,
//     total_time: f32,
//     ui_sliders: PlayerUiSliders,
//     ui_sliders_drag: PlayerUiSliders,
//     pub(crate) detect_features: bool,
//     pub(crate) buffer_limit: Option<u32>,
// }
//
// impl Default for PlayerUiState {
//     fn default() -> Self {
//         Self {
//             playing: true,
//             looping: true,
//             view_mode: FramedViewMode::Intensity,
//             reconstruction_method: ReconstructionMethod::Accurate,
//             current_frame: 0,
//             total_frames: 0,
//             current_time: 0.0,
//             total_time: 0.0,
//             ui_sliders: Default::default(),
//             ui_sliders_drag: Default::default(),
//             detect_features: false,
//             buffer_limit: Some(60),
//         }
//     }
// }
//
// pub struct InfoUiState {
//     stream_state: StreamState,
//     events_per_sec: f64,
//     events_ppc_per_sec: f64,
//     events_ppc_total: f64,
//     events_total: u64,
//     event_size: u8,
//     plane: PlaneSize,
//     source_name: RichText,
//     pub(crate) plot_points_raw_adder_bitrate_y: PlotY,
// }
//
// impl Default for InfoUiState {
//     fn default() -> Self {
//         let plot_points: VecDeque<Option<f64>> = (0..1000).map(|_| None).collect();
//
//         InfoUiState {
//             stream_state: Default::default(),
//             events_per_sec: 0.,
//             events_ppc_per_sec: 0.,
//             events_ppc_total: 0.0,
//             events_total: 0,
//             event_size: 0,
//             plane: Default::default(),
//             source_name: RichText::new("No file selected yet"),
//             plot_points_raw_adder_bitrate_y: PlotY {
//                 points: plot_points.clone(),
//             },
//         }
//     }
// }
//
// impl InfoUiState {
//     fn _clear_stats(&mut self) {
//         self.events_per_sec = 0.;
//         self.events_ppc_per_sec = 0.;
//         self.events_ppc_total = 0.0;
//         self.events_total = 0;
//     }
// }
//
// #[derive(Resource, Default)]
// pub struct PlayerState {
//     player_rx: Option<Receiver<PlayerStreamArtifact>>,
//     player_path_buf: Option<PathBuf>,
//     ui_state: PlayerUiState,
//     pub(crate) ui_info_state: InfoUiState,
// }
//
// unsafe impl Sync for PlayerState {}
//
// impl PlayerState {
//     pub fn consume_source(
//         &mut self,
//         mut images: ResMut<Assets<Image>>,
//         mut handles: ResMut<Images>,
//     ) -> Result<(), Box<dyn Error>> {
//         if !self.ui_state.playing {
//             return Ok(());
//         }
//         if let Some(rx) = &self.player_rx {
//             let (event_count, stream_state, image_opt) = rx.try_recv()?;
//             self.ui_info_state.events_total += event_count;
//             self.ui_info_state.stream_state = stream_state;
//
//             if let Some(image) = image_opt {
//                 images.remove(&handles.image_view);
//                 let handle = images.add(image);
//                 handles.image_view = handle;
//             } else if self.ui_info_state.stream_state.file_pos == 1 {
//                 dbg!("Looping...");
//                 self.reset_update_adder_params(true);
//
//                 return Ok(());
//             }
//             return Ok(());
//         }
//         Err("".into())
//     }
//
//     pub fn play(&mut self) {
//         self.ui_state.playing = true;
//     }
//
//     // Fill in the side panel with sliders for playback speed and buttons for play/pause/stop
//     pub fn side_panel_ui(
//         &mut self,
//         ui: &mut Ui,
//         mut commands: Commands,
//         _images: &mut ResMut<Assets<Image>>,
//     ) {
//         ui.horizontal(|ui| {
//             ui.heading("ADΔER Parameters");
//             if ui.add(egui::Button::new("Reset params")).clicked() {
//                 self.ui_state = Default::default();
//                 self.ui_state.ui_sliders = Default::default();
//                 if self.ui_state.ui_sliders_drag != self.ui_state.ui_sliders {
//                     self.reset_update_adder_params(true)
//                 }
//                 self.ui_state.ui_sliders_drag = Default::default();
//             }
//             if ui.add(egui::Button::new("Reset video")).clicked() {
//                 // self.player = AdderPlayer::default();
//                 self.ui_state = Default::default();
//                 self.ui_state.ui_sliders = Default::default();
//                 self.ui_state.ui_sliders_drag = Default::default();
//                 self.ui_info_state = Default::default();
//                 self.reset_update_adder_params(false);
//                 commands.insert_resource(Images::default());
//             }
//         });
//         egui::Grid::new("my_grid")
//             .num_columns(2)
//             .spacing([10.0, 4.0])
//             .striped(true)
//             .show(ui, |ui| {
//                 self.side_panel_grid_contents(ui);
//             });
//     }
//
//     pub fn side_panel_grid_contents(&mut self, ui: &mut Ui) {
//         let mut need_to_update = add_slider_row(
//             true,
//             true,
//             "Playback speed:",
//             ui,
//             &mut self.ui_state.ui_sliders.playback_speed,
//             &mut self.ui_state.ui_sliders_drag.playback_speed,
//             0.1..=10000.0,
//             vec![1.0, 2.0, 5.0, 10.0],
//             0.1,
//         );
//
//         // TODO!
//         // match &self.player.input_stream {
//         //     None => {}
//         //     Some(stream) => {
//         //         let duration = Duration::from_nanos(
//         //             ((self.player.current_t_ticks as f64 / stream.tps as f64) * 1.0e9) as u64,
//         //         );
//         //         ui.add_enabled(true, egui::Label::new("Current time:"));
//         //         ui.add_enabled(true, egui::Label::new(to_string(duration)));
//         //         ui.end_row();
//         //     }
//         // }
//
//         ui.add_enabled(true, egui::Label::new("Playback controls:"));
//         ui.horizontal(|ui| {
//             if self.ui_state.playing {
//                 if ui.button("⏸").clicked() {
//                     self.ui_state.playing = false;
//                 }
//             } else if ui.button("▶").clicked() {
//                 self.ui_state.playing = true;
//             }
//             // TODO: remove this?
//             if ui.button("⏹").clicked() {
//                 self.ui_state.playing = false;
//                 need_to_update = true;
//             }
//
//             if ui.button("⏮").clicked() {
//                 self.ui_state.playing = true;
//                 self.ui_info_state.stream_state.file_pos = 0; // To force the player to restart
//                 need_to_update = true;
//             }
//         });
//         ui.end_row();
//
//         // TODO: decoding is single-threaded for now
//         add_slider_row(
//             false,
//             false,
//             "Thread count:",
//             ui,
//             &mut self.ui_state.ui_sliders.thread_count,
//             &mut self.ui_state.ui_sliders_drag.thread_count,
//             1..=(current_num_threads() - 1).max(4),
//             vec![],
//             1,
//         );
//         add_checkbox_row(
//             true,
//             "Loop:",
//             "Loop playback?",
//             ui,
//             &mut self.ui_state.looping,
//         ); // TODO: add more sliders
//
//         // TODO
//         need_to_update |= add_radio_row(
//             true,
//             "View mode:",
//             vec![
//                 ("Intensity", FramedViewMode::Intensity),
//                 ("D", FramedViewMode::D),
//                 ("Δt", FramedViewMode::DeltaT),
//                 ("SAE", FramedViewMode::SAE),
//             ],
//             ui,
//             &mut self.ui_state.view_mode,
//         );
//         need_to_update |= add_radio_row(
//             true,
//             "Reconstruction method:",
//             vec![
//                 ("Fast", ReconstructionMethod::Fast),
//                 ("Accurate", ReconstructionMethod::Accurate),
//             ],
//             ui,
//             &mut self.ui_state.reconstruction_method,
//         );
//
//         let mut limit_frame_buffer_bool = self.ui_state.buffer_limit.is_some();
//         need_to_update |= add_checkbox_row(
//             true,
//             "Frame buffer",
//             "Limit frame buffer?",
//             ui,
//             &mut limit_frame_buffer_bool,
//         );
//         if limit_frame_buffer_bool && self.ui_state.buffer_limit.is_none() {
//             self.ui_state.buffer_limit = Some(100);
//         } else if !limit_frame_buffer_bool {
//             self.ui_state.buffer_limit = None;
//         }
//
//         let mut buffer_limit = self.ui_state.buffer_limit.unwrap_or(100);
//         let mut buffer_limit_tmp = buffer_limit;
//         need_to_update |= add_slider_row(
//             limit_frame_buffer_bool,
//             false,
//             "Buffer limit:",
//             ui,
//             &mut buffer_limit,
//             &mut buffer_limit_tmp,
//             0..=1000,
//             vec![10, 100, 250, 500, 750],
//             10,
//         );
//
//         if limit_frame_buffer_bool
//             && (buffer_limit != buffer_limit_tmp
//                 || self.ui_state.buffer_limit != Some(buffer_limit_tmp))
//         {
//             self.ui_state.buffer_limit = Some(buffer_limit_tmp);
//             need_to_update = true;
//         }
//
//         ui.label("Processing:");
//         need_to_update |= ui
//             .add_enabled(
//                 true,
//                 egui::Checkbox::new(&mut self.ui_state.detect_features, "Detect features"),
//             )
//             .changed();
//         ui.end_row();
//
//         if need_to_update {
//             self.reset_update_adder_params(true)
//         }
//     }
//
//     pub fn central_panel_ui(&mut self, ui: &mut Ui, time: Res<Time>) {
//         ui.horizontal(|ui| {
//             if ui.button("Open file").clicked() {
//                 if let Some(path) = rfd::FileDialog::new()
//                     .add_filter("adder video", &["adder"])
//                     .pick_file()
//                 {
//                     self.player_path_buf = Some(path.clone());
//                     self.replace_player(&path);
//                 }
//             }
//
//             ui.label("OR drag and drop your ADΔER file here (.adder)");
//         });
//
//         ui.label(self.ui_info_state.source_name.clone());
//
//         let duration_secs = self.ui_info_state.stream_state.current_t_ticks as f64
//             / self.ui_info_state.stream_state.tps as f64;
//         self.ui_info_state.events_per_sec = self.ui_info_state.events_total as f64 / duration_secs;
//         self.ui_info_state.events_ppc_total =
//             self.ui_info_state.events_total as f64 / self.ui_info_state.stream_state.volume as f64;
//         self.ui_info_state.events_ppc_per_sec = self.ui_info_state.events_ppc_total / duration_secs;
//
//         let bitrate = self.ui_info_state.events_ppc_per_sec
//             * self.ui_info_state.event_size as f64
//             * self.ui_info_state.plane.volume() as f64
//             / 1024.0
//             / 1024.0; // transcoded raw in megabytes per sec
//         self.ui_info_state
//             .plot_points_raw_adder_bitrate_y
//             .update(Some(bitrate));
//
//         // TODO: make fps accurate and meaningful here
//         ui.label(format!(
//             "{:.2} transcoded FPS\t\
//             {:.2} events per source sec\t\
//             {:.2} events PPC per source sec\t\
//             {:.0} events total\t\
//             {:.0} events PPC total
//             ",
//             1. / time.delta_seconds(),
//             self.ui_info_state.events_per_sec,
//             self.ui_info_state.events_ppc_per_sec,
//             self.ui_info_state.events_total,
//             self.ui_info_state.events_ppc_total
//         ));
//     }
//
//     fn reset_update_adder_params(&mut self, replace_player: bool) {
//         self.ui_state.current_frame = match self.ui_state.reconstruction_method {
//             ReconstructionMethod::Fast => 1,
//             ReconstructionMethod::Accurate => 0,
//         };
//         self.ui_state.total_frames = 0;
//         self.ui_state.current_time = 0.0;
//         self.ui_state.total_time = 0.0;
//
//         let path_buf = match &self.player_path_buf {
//             None => {
//                 return;
//             }
//             Some(p) => p.clone(),
//         };
//
//         if replace_player {
//             self.replace_player(&path_buf);
//         } else {
//             self.player_path_buf = None;
//             self.player_rx = None;
//         }
//     }
//
//     pub fn replace_player(&mut self, path_buf: &std::path::Path) {
//         self.player_path_buf = Some(PathBuf::from(path_buf));
//         self.ui_info_state.events_total = 0;
//         self.ui_info_state.events_ppc_total = 0.0;
//         let mut player = match AdderPlayer::new(
//             path_buf,
//             self.ui_state.ui_sliders.playback_speed,
//             self.ui_state.view_mode,
//             self.ui_state.detect_features,
//             self.ui_state.buffer_limit,
//         ) {
//             Ok(player) => {
//                 self.ui_info_state.source_name = RichText::from(match path_buf.to_str() {
//                     None => "Error: couldn't get path string".to_string(),
//                     Some(path) => path.to_string(),
//                 })
//                 .color(Color32::DARK_GREEN);
//                 player
//             }
//             Err(e) => {
//                 self.ui_info_state.source_name = RichText::new(e.to_string()).color(Color32::RED);
//                 return;
//             }
//         };
//
//         player = player.reconstruction_method(self.ui_state.reconstruction_method.clone());
//         // player = player.stream_pos(self.ui_info_state.stream_state.file_pos);
//         // TODO: Restore
//         player = player.stream_pos(0);
//
//         let plane = player.input_stream.as_ref().unwrap().decoder.meta().plane;
//         self.ui_info_state.event_size = if plane.c() == 1 { 9 } else { 11 };
//         self.ui_info_state.plane = plane;
//
//         self.ui_state.current_frame = 1;
//
//         let (player_tx, player_rx) = bounded(60);
//         let detect_features = self.ui_state.detect_features;
//
//         rayon::spawn(move || loop {
//             let res = player.consume_source(detect_features);
//             match player_tx.send(res) {
//                 Ok(_) => {}
//                 Err(_) => {
//                     break;
//                 }
//             };
//         });
//
//         self.player_rx = Some(player_rx);
//     }
// }
//
// fn _to_string(duration: Duration) -> String {
//     let hours = duration.as_secs() / 3600;
//     let mins = (duration.as_secs() % 3600) / 60;
//     let secs = duration.as_secs() % 60;
//     let nanos = duration.subsec_nanos();
//     format!("{hours}:{mins}:{secs}.{nanos:09}")
// }
