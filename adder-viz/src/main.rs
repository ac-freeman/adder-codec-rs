mod player;
mod transcoder;
mod utils;

use crate::player::ui::PlayerUi;
use crate::transcoder::ui::TranscoderUi;
use eframe::egui;
use egui::{ColorImage, Ui, Widget, WidgetText};
use strum::IntoEnumIterator;
use strum_macros::EnumIter;

fn main() {
    let native_options = eframe::NativeOptions::default();
    eframe::run_native(
        "ADΔER Viz",
        native_options,
        Box::new(|cc| Box::new(App::new(cc))),
    );
}

struct App {
    view: Tabs,
    error_msg: Option<String>,
    transcoder_ui: TranscoderUi,
    player_ui: PlayerUi,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // Customize egui here with cc.egui_ctx.set_fonts and cc.egui_ctx.set_visuals.
        // Restore app state using cc.storage (requires the "persistence" feature).
        // Use the cc.gl (a glow::Context) to create graphics shaders and buffers that you can use
        // for e.g. egui::PaintCallback.
        cc.egui_ctx.set_visuals(egui::Visuals {
            window_rounding: 5.0.into(),
            ..Default::default()
        });

        Self {
            view: Default::default(),
            error_msg: None,

            transcoder_ui: TranscoderUi::new(cc),
            player_ui: PlayerUi::new(cc),
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        // self.handle_exit(ctx);

        // Check if the scale key was hit
        handle_zoom(ctx);
        configure_menu_bar(self, ctx);

        match self.view {
            Tabs::Transcoder => self.transcoder_ui.update(ctx),
            Tabs::Player => self.player_ui.update(ctx),
        }

        ctx.request_repaint();
    }
}

fn handle_zoom(ctx: &egui::Context) {
    if ctx.input(|i| i.key_pressed(egui::Key::Slash)) {
        // Toggle the scale factor
        let scale_factor = if ctx.zoom_factor() == 1.0_f32 {
            2.0
        } else {
            1.0
        };
        ctx.set_zoom_factor(scale_factor);
    }
}

// mod player;
// mod transcoder;
// mod utils;
//
// use egui::epaint::{
//     text::{LayoutJob, TextFormat},
//     Color32, FontFamily, FontId,
// };
// use std::ops::RangeInclusive;
//
// use crate::player::ui::PlayerState;
// use crate::transcoder::ui::TranscoderState;
//
// // use egui_dock::egui as dock_egui;
// use egui::{emath, global_dark_light_mode_switch, Align, Rounding, Ui, Widget, WidgetText};
//
// use crate::transcoder::adder::replace_adder_transcoder;
// use strum::IntoEnumIterator;
// use strum_macros::EnumIter;
//
// // use egui_dock::{NodeIndex, Tree};
//
#[derive(Default, Debug, EnumIter, PartialEq, Clone, Copy)]
enum Tabs {
    #[default]
    Transcoder,
    Player,
}

impl Tabs {
    fn as_str(&self) -> &'static str {
        match self {
            Tabs::Transcoder => "Transcode",
            Tabs::Player => "Play file",
        }
    }
}

#[derive(Default)]
pub struct Images {
    input_view: Option<ColorImage>,
    image_view: Option<ColorImage>,
}

/// Draw the menu bar (the tabs at the top of the window)
fn configure_menu_bar(app: &mut App, ctx: &egui::Context) {
    let style = ctx.style();

    egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
        egui::menu::bar(ui, |ui| {
            egui::global_dark_light_mode_switch(ui);

            ui.style_mut().visuals.widgets.active.rounding = egui::Rounding::same(0.0);
            let inactive_tab_text_stroke = egui::Stroke {
                width: Default::default(),
                color: egui::Color32::DARK_GRAY,
            };
            let active_tab_text_stroke = egui::Stroke {
                width: Default::default(),
                color: egui::Color32::WHITE,
            };
            ui.style_mut().visuals.widgets.inactive.rounding = egui::Rounding::same(0.0);
            ui.style_mut().visuals.widgets.open.rounding = egui::Rounding::same(0.0);
            ui.style_mut().visuals.widgets.hovered.rounding = egui::Rounding::same(0.0);
            ui.style_mut().visuals.widgets.noninteractive.rounding = egui::Rounding::same(0.0);
            ui.style_mut().visuals.widgets.inactive.expansion = 3.0;
            ui.style_mut().visuals.widgets.active.expansion = 3.0;
            ui.style_mut().visuals.widgets.hovered.expansion = 3.0;

            let mut new_selection = app.view;
            for menu_item in Tabs::iter() {
                let button = {
                    if app.view == menu_item {
                        ui.style_mut().visuals.widgets.inactive.fg_stroke = active_tab_text_stroke;
                        egui::Button::new(menu_item.as_str()).fill(style.visuals.window_fill)
                    } else {
                        ui.style_mut().visuals.widgets.inactive.fg_stroke =
                            inactive_tab_text_stroke;
                        egui::Button::new(menu_item.as_str()).fill(style.visuals.faint_bg_color)
                    }
                };
                let res = button.ui(ui);
                if res.clicked() {
                    new_selection = menu_item;
                }
            }

            // Now that all the menu items have been drawn, set the selected item for when the next
            // frame is drawn
            if app.view != new_selection {
                // Clear the image vec
                // images.clear();
                app.view = new_selection;
            }
        });
    });
}

// trait VizTab<A,B> {
//     fn new(cc: &eframe::CreationContext<'_>) -> Self;
//
//     fn spawn_tab_runner(&mut self,
//                         rx: mpsc::Receiver<A>,
//                         msg_tx: mpsc::Sender<B>,);
//
//     fn update(&mut self, ctx: &egui::Context);
//
//     /// If the user has dropped a file into the window, we store the file path.
//     /// At the end of the frame, the receiver will be notified by update()
//     fn handle_file_drop(&mut self, ctx: &egui::Context);
// }

trait VizUi {
    fn draw_ui(&mut self, ctx: &egui::Context);

    fn side_panel_ui(&mut self, ui: &mut egui::Ui);

    fn central_panel_ui(&mut self, ui: &mut egui::Ui);

    fn side_panel_grid_contents(&mut self, ui: &mut egui::Ui);
}

trait TabState {
    fn reset_params(&mut self);

    fn reset_video(&mut self);
}

//
// #[allow(clippy::too_many_arguments)]
// fn draw_ui(
//     app: &mut App,
//     ctx: &egui::Context, // mut transcoder_state: ResMut<TranscoderState>,
//                          // mut player_state: ResMut<PlayerState>,
//                          // main_ui_state: Res<MainUiState>,
// ) {
//     egui::SidePanel::left("side_panel")
//         .default_width(300.0)
//         .show(ctx, |ui| {
//             ui.label(format!(
//                 "FPS: {:.2}",
//                 1.0 / app.last_frame_time.elapsed().as_secs_f64()
//             ));
//             // update the last frame time
//             app.last_frame_time = std::time::Instant::now();
//
//             match app.view {
//                 Tabs::Transcoder => {
//                     app.transcoder_ui.side_panel_ui(ui);
//                 }
//                 Tabs::Player => {
//                     // player_state.side_panel_ui(ui, commands, &mut images);
//                 }
//             }
//         });
//
//     egui::CentralPanel::default().show(ctx, |ui| {
//         egui::warn_if_debug_build(ui);
//
//         match app.view {
//             Tabs::Transcoder => {
//                 app.transcoder_ui.central_panel_ui(
//                     ui,
//                     &mut app.input_image_handle,
//                     &mut app.adder_image_handle,
//                 );
//             }
//             Tabs::Player => {
//                 // app.player_state.central_panel_ui(ui, time);
//             }
//         }

/*
Images in the central panel are common to both visualization tabs, so we can do this
 here as the last step of drawing its UI
*/
// let mut has_input = false;
// let avail_size = ui.available_size();
// ui.horizontal(|ui| {
//     ui.set_max_size(avail_size);
//
//     ui.vertical(|ui| {
//                 if let (Some(input), Some(input_texture_id)) = (input, input_texture_id) {
//                     let mut avail_size = ui.available_size();
//                     avail_size.x = avail_size.x / 2.0 - ui.spacing().item_spacing.y / 2.0;
//
//                     ui.set_max_size(avail_size);
//
//                     // Right-align the text so it's easier to compare to the ADDER version
//                     ui.with_layout(egui::Layout::top_down(Align::Max), |ui| {
//                         let mut job = LayoutJob::default();
//                         job.append(
//                             "Input\n",
//                             0.0,
//                             TextFormat {
//                                 font_id: FontId::new(24.0, FontFamily::Proportional),
//                                 color: Color32::WHITE,
//                                 ..Default::default()
//                             },
//                         );
//
//                         let last = transcoder_state
//                             .ui_info_state
//                             .plot_points_raw_source_bitrate_y
//                             .points
//                             .iter()
//                             .last();
//                         let str_num = match last {
//                             None => -999.0,
//                             Some(item) => item.unwrap_or(-999.0),
//                         };
//
//                         let str = format!("{number:.prec$} MB/s", prec = 2, number = str_num);
//                         job.append(
//                             &str,
//                             0.0,
//                             TextFormat {
//                                 font_id: FontId::new(14.0, FontFamily::Proportional),
//                                 ..Default::default()
//                             },
//                         );
//                         ui.label(job);
//                     });
//                     has_input = true;
//
//                     let size = match (
//                         input.texture_descriptor.size.width as f32,
//                         input.texture_descriptor.size.height as f32,
//                     ) {
//                         (a, b) if a / b > avail_size.x / avail_size.y => {
//                             /*
//                             The available space has a taller aspect ratio than the video
//                             Fill the available horizontal space.
//                              */
//                             egui::Vec2 {
//                                 x: avail_size.x,
//                                 y: (avail_size.x / a) * b,
//                             }
//                         }
//                         (a, b) => {
//                             /*
//                             The available space has a shorter aspect ratio than the video
//                             Fill the available vertical space.
//                              */
//                             egui::Vec2 {
//                                 x: (avail_size.y / b) * a,
//                                 y: avail_size.y,
//                             }
//                         }
//                     };
//                     ui.image(input_texture_id, size);
//                 }
//             });
//
//             ui.vertical(|ui| {
//                 if let (Some(image), Some(texture_id)) = (image, texture_id) {
//                     ui.with_layout(egui::Layout::top_down(Align::Min), |ui| {
//                         let mut job = LayoutJob::default();
//                         job.append(
//                             "ADΔER\n",
//                             0.0,
//                             TextFormat {
//                                 font_id: FontId::new(24.0, FontFamily::Proportional),
//                                 color: Color32::WHITE,
//                                 ..Default::default()
//                             },
//                         );
//
//                         let (bitrate, percentage_str, color) = match main_ui_state.view {
//                             Tabs::Transcoder => {
//                                 let last = transcoder_state
//                                     .ui_info_state
//                                     .plot_points_raw_adder_bitrate_y
//                                     .points
//                                     .iter()
//                                     .last();
//                                 let adder_bitrate = match last {
//                                     None => -999.0,
//                                     Some(item) => item.unwrap_or(-999.0),
//                                 };
//
//                                 let last = transcoder_state
//                                     .ui_info_state
//                                     .plot_points_raw_source_bitrate_y
//                                     .points
//                                     .iter()
//                                     .last();
//                                 let source_bitrate = match last {
//                                     None => -999.0,
//                                     Some(item) => item.unwrap_or(-999.0),
//                                 };
//
//                                 let percentage = adder_bitrate / source_bitrate * 100.0;
//
//                                 let percentage_str =
//                                     format!("{number:.prec$}%", prec = 2, number = percentage);
//                                 let color = if percentage < 100.0 {
//                                     Color32::GREEN
//                                 } else {
//                                     Color32::RED
//                                 };
//                                 (adder_bitrate, percentage_str, color)
//                             }
//                             Tabs::Player => {
//                                 let last = transcoder_state
//                                     .ui_info_state
//                                     .plot_points_raw_adder_bitrate_y
//                                     .points
//                                     .iter()
//                                     .last();
//                                 let adder_bitrate = match last {
//                                     None => -999.0,
//                                     Some(item) => item.unwrap_or(-999.0),
//                                 };
//                                 (adder_bitrate, "".to_string(), Color32::WHITE)
//                             }
//                         };
//
//                         let str = format!("{number:.prec$} MB/s | ", prec = 2, number = bitrate);
//                         job.append(
//                             &str,
//                             0.0,
//                             TextFormat {
//                                 font_id: FontId::new(14.0, FontFamily::Proportional),
//                                 ..Default::default()
//                             },
//                         );
//
//                         job.append(
//                             &percentage_str,
//                             0.0,
//                             TextFormat {
//                                 font_id: FontId::new(14.0, FontFamily::Proportional),
//                                 color,
//                                 ..Default::default()
//                             },
//                         );
//
//                         ui.label(job);
//                     });
//
//                     let avail_size = ui.available_size();
//                     let size = match (
//                         image.texture_descriptor.size.width as f32,
//                         image.texture_descriptor.size.height as f32,
//                     ) {
//                         (a, b) if a / b > avail_size.x / avail_size.y => {
//                             /*
//                             The available space has a taller aspect ratio than the video
//                             Fill the available horizontal space.
//                              */
//                             egui::Vec2 {
//                                 x: avail_size.x,
//                                 y: (avail_size.x / a) * b,
//                             }
//                         }
//                         (a, b) => {
//                             /*
//                             The available space has a shorter aspect ratio than the video
//                             Fill the available vertical space.
//                              */
//                             egui::Vec2 {
//                                 x: (avail_size.y / b) * a,
//                                 y: avail_size.y,
//                             }
//                         }
//                     };
//
//                     ui.image(texture_id, size);
//                 }
//             });
//         });
//
//         if let Some(msg) = main_ui_state.error_msg.as_ref() {
//             ui.label(msg);
//         }
//     });
// }
//
// fn update_adder_params(
//     main_ui_state: Res<MainUiState>,
//     handles: Res<Images>,
//     images: ResMut<Assets<Image>>,
//     mut transcoder_state: ResMut<TranscoderState>,
// ) {
//     match main_ui_state.view {
//         Tabs::Transcoder => {
//             transcoder_state.update_adder_params(handles, images);
//         }
//         Tabs::Player => {
//             // player_state.update_adder_params(commands);
//         }
//     }
// }
//
// fn consume_source(
//     images: ResMut<Assets<Image>>,
//     handles: ResMut<Images>,
//     mut main_ui_state: ResMut<MainUiState>,
//     mut transcoder_state: ResMut<TranscoderState>,
//     mut player_state: ResMut<PlayerState>,
// ) {
//     let res = match main_ui_state.view {
//         Tabs::Transcoder => transcoder_state.consume_source(images, handles),
//         Tabs::Player => player_state.consume_source(images, handles),
//     };
//
//     match res {
//         Ok(_) => {}
//         Err(e) => {
//             if e.is::<std::sync::mpsc::TryRecvError>() {
//                 main_ui_state.error_msg = Some("Loading file...".to_string());
//             } else {
//                 main_ui_state.error_msg = Some(format!("{e}"));
//             }
//         }
//     }
// }
//
// #[derive(Component, Default)]
// struct MyDropTarget;
//
// ///<https://bevy-cheatbook.github.io/input/dnd.html>
// fn file_drop(
//     main_ui_state: ResMut<MainUiState>,
//     mut player_state: ResMut<PlayerState>,
//     mut transcoder_state: ResMut<TranscoderState>,
//     mut dnd_evr: EventReader<FileDragAndDrop>,
//     query_ui_droptarget: Query<&Interaction, With<MyDropTarget>>,
// ) {
//     for ev in dnd_evr.iter() {
//         if let FileDragAndDrop::DroppedFile { path_buf, .. } = ev {
//             for interaction in query_ui_droptarget.iter() {
//                 if *interaction == Interaction::Hovered {
//                     // it was dropped over our UI element
//                     // (our UI element is being hovered over)
//                 }
//             }
//
//             match main_ui_state.view {
//                 Tabs::Transcoder => {
//                     transcoder_state.ui_info_state.input_path_0 = Some(path_buf.clone());
//                     transcoder_state.ui_info_state.input_path_1 = None;
//
//                     let output_path_opt = transcoder_state.ui_info_state.output_path.clone();
//                     // TODO: refactor as struct func
//                     replace_adder_transcoder(
//                         &mut transcoder_state,
//                         Some(path_buf.clone()),
//                         None,
//                         output_path_opt,
//                         0,
//                     ); // TODO!!
//                 }
//                 Tabs::Player => {
//                     player_state.replace_player(path_buf);
//                     player_state.play();
//                 }
//             }
//         }
//     }
// }
//

fn add_radio_row<Value: PartialEq + Clone>(
    enabled: bool,
    label: impl Into<WidgetText>,
    options: Vec<(impl Into<WidgetText> + Clone, Value)>,
    ui: &mut Ui,
    radio_state: &mut Value,
) -> bool {
    ui.label(label);
    let mut ret = false;
    ui.add_enabled_ui(enabled, |ui| {
        ui.horizontal(|ui| {
            for option in options {
                ret |= ui
                    .radio_value(radio_state, option.1.clone(), option.0.clone())
                    .changed();
            }
        });
    });
    ui.end_row();
    ret
}

//

//

//
//
//
trait Pm {
    fn increment(&mut self, bound: &Self, interval: &Self);
    fn decrement(&mut self, bound: &Self, interval: &Self);
}

macro_rules! impl_pm_float {
    ($t: ident) => {
        impl Pm for $t {
            #[inline(always)]
            fn increment(&mut self, bound: &Self, interval: &Self) {
                #[allow(trivial_numeric_casts)]
                {
                    *self += *interval;
                    if *self > *bound {
                        *self = *bound
                    }
                }
            }

            #[inline(always)]
            fn decrement(&mut self, bound: &Self, interval: &Self) {
                #[allow(trivial_numeric_casts)]
                {
                    *self -= *interval;
                    if *self < *bound {
                        *self = *bound
                    }
                }
            }
        }
    };
}
macro_rules! impl_pm_integer {
    ($t: ident) => {
        impl Pm for $t {
            #[inline(always)]
            fn increment(&mut self, bound: &Self, interval: &Self) {
                #[allow(trivial_numeric_casts)]
                {
                    *self = self.saturating_add(*interval);
                    if *self > *bound {
                        *self = *bound
                    }
                }
            }

            #[inline(always)]
            fn decrement(&mut self, bound: &Self, interval: &Self) {
                #[allow(trivial_numeric_casts)]
                {
                    *self = self.saturating_sub(*interval);
                    if *self < *bound {
                        *self = *bound
                    }
                }
            }
        }
    };
}

impl_pm_float!(f32);
impl_pm_float!(f64);
impl_pm_integer!(i8);
impl_pm_integer!(u8);
impl_pm_integer!(i16);
impl_pm_integer!(u16);
impl_pm_integer!(i32);
impl_pm_integer!(u32);
impl_pm_integer!(i64);
impl_pm_integer!(u64);
impl_pm_integer!(isize);
impl_pm_integer!(usize);
