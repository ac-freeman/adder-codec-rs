mod player;
mod transcoder;
mod utils;

use bevy_egui::egui::epaint::{
    text::{LayoutJob, TextFormat},
    Color32, FontFamily, FontId,
};
use std::ops::RangeInclusive;

use crate::player::ui::PlayerState;
use crate::transcoder::ui::TranscoderState;
use bevy::ecs::system::Resource;
use bevy::prelude::*;
use bevy::window::{PresentMode, PrimaryWindow, WindowResolution};

use bevy_egui::{egui, EguiContexts, EguiPlugin, EguiSettings};
// use egui_dock::egui as dock_egui;
use bevy_egui::egui::{
    emath, global_dark_light_mode_switch, Align, Rounding, Ui, Widget, WidgetText,
};

use crate::transcoder::adder::replace_adder_transcoder;
use strum::IntoEnumIterator;
use strum_macros::EnumIter;

// use egui_dock::{NodeIndex, Tree};

#[derive(Debug, EnumIter, PartialEq, Clone, Copy)]
enum Tabs {
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

#[derive(Resource)]
pub struct MainUiState {
    view: Tabs,
    error_msg: Option<String>,
}

use crate::utils::slider::NotchedSlider;

/// This example demonstrates the following functionality and use-cases of bevy_egui:
/// - rendering loaded assets;
/// - toggling hidpi scaling (by pressing '/' button);
/// - configuring egui contexts during the startup.
fn main() {
    App::new()
        .insert_resource(ClearColor(Color::rgb(0.0, 0.0, 0.0)))
        .insert_resource(Msaa::default())
        .insert_resource(Images::default())
        .insert_resource(MainUiState {
            view: Tabs::Transcoder,
            error_msg: None,
        })
        .init_resource::<TranscoderState>()
        .init_resource::<PlayerState>()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "ADΔER Viz".to_string(),
                resolution: WindowResolution::default(),
                present_mode: PresentMode::AutoVsync,
                ..default()
            }),
            ..default()
        }))
        .add_plugins(EguiPlugin)
        .add_systems(Update, configure_menu_bar.before(draw_ui))
        .add_systems(Startup, configure_visuals)
        .add_systems(Update, update_ui_scale_factor)
        .add_systems(Update, draw_ui)
        .add_systems(Update, file_drop)
        .add_systems(Update, update_adder_params)
        .add_systems(Update, consume_source)
        .run();
}

#[derive(Resource, Default)]
pub struct Images {
    last_image_view: Handle<Image>,
    image_view: Handle<Image>,
    input_view: Handle<Image>,
}

fn configure_visuals(mut egui_ctx: EguiContexts) {
    egui_ctx.ctx_mut().set_visuals(bevy_egui::egui::Visuals {
        window_rounding: 5.0.into(),
        ..Default::default()
    });
}

fn update_ui_scale_factor(
    keyboard_input: Res<Input<KeyCode>>,
    mut toggle_scale_factor: Local<Option<bool>>,
    mut egui_settings: ResMut<EguiSettings>,
    windows: Query<&Window, With<PrimaryWindow>>,
) {
    if keyboard_input.just_pressed(KeyCode::Slash) || toggle_scale_factor.is_none() {
        *toggle_scale_factor = Some(!toggle_scale_factor.unwrap_or(false));

        if let Ok(window) = windows.get_single() {
            let scale_factor = if toggle_scale_factor.unwrap_or(true) {
                1.0
            } else {
                eprintln!(
                    "Primary window found, using scale factor: {}",
                    window.scale_factor()
                );
                1.0 / window.scale_factor()
            };
            egui_settings.scale_factor = scale_factor;
        }
    }
}

fn configure_menu_bar(
    mut main_ui_state: ResMut<MainUiState>,
    mut egui_ctx: EguiContexts,
    mut images: ResMut<Assets<Image>>,
) {
    let style = (*(egui_ctx).ctx_mut().clone().style()).clone();

    egui::TopBottomPanel::top("top_panel").show(egui_ctx.ctx_mut(), |ui| {
        egui::menu::bar(ui, |ui| {
            global_dark_light_mode_switch(ui);

            ui.style_mut().visuals.widgets.active.rounding = Rounding::same(0.0);
            ui.style_mut().visuals.widgets.inactive.rounding = Rounding::same(0.0);
            ui.style_mut().visuals.widgets.open.rounding = Rounding::same(0.0);
            ui.style_mut().visuals.widgets.hovered.rounding = Rounding::same(0.0);
            ui.style_mut().visuals.widgets.noninteractive.rounding = Rounding::same(0.0);
            ui.style_mut().visuals.widgets.inactive.expansion = 3.0;
            ui.style_mut().visuals.widgets.active.expansion = 3.0;
            ui.style_mut().visuals.widgets.hovered.expansion = 3.0;
            let default_inactive_stroke = ui.style_mut().visuals.widgets.inactive.fg_stroke;

            let mut new_selection = main_ui_state.view;
            for menu_item in Tabs::iter() {
                let button = {
                    if main_ui_state.view == menu_item {
                        ui.style_mut().visuals.widgets.inactive.fg_stroke =
                            ui.style_mut().visuals.widgets.active.fg_stroke;
                        egui::Button::new(menu_item.as_str()).fill(style.visuals.window_fill)
                    } else {
                        ui.style_mut().visuals.widgets.inactive.fg_stroke = default_inactive_stroke;
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
            if main_ui_state.view != new_selection {
                // Clear the image vec
                images.clear();
                main_ui_state.view = new_selection;
            }
        });
    });
}

#[allow(clippy::too_many_arguments)]
fn draw_ui(
    commands: Commands,
    time: Res<Time>, // Time passed since last frame
    handles: Res<Images>,
    mut images: ResMut<Assets<Image>>,
    mut egui_ctx: EguiContexts,
    mut transcoder_state: ResMut<TranscoderState>,
    mut player_state: ResMut<PlayerState>,
    main_ui_state: Res<MainUiState>,
) {
    egui::SidePanel::left("side_panel")
        .default_width(300.0)
        .show(egui_ctx.ctx_mut(), |ui| match main_ui_state.view {
            Tabs::Transcoder => {
                transcoder_state.side_panel_ui(ui, commands, &mut images);
            }
            Tabs::Player => {
                player_state.side_panel_ui(ui, commands, &mut images);
            }
        });

    images.remove(&handles.last_image_view);

    let (image, texture_id) = match images.get(&handles.image_view) {
        // texture_id = Some(egui_ctx.add_image(handles.image_view.clone()));
        None => (None, None),
        Some(image) => (
            Some(image),
            Some(egui_ctx.add_image(handles.image_view.clone())),
        ),
    };

    let (input, input_texture_id) = match images.get(&handles.input_view) {
        // texture_id = Some(egui_ctx.add_image(handles.image_view.clone()));
        None => (None, None),
        Some(image) => (
            Some(image),
            Some(egui_ctx.add_image(handles.input_view.clone())),
        ),
    };

    egui::CentralPanel::default().show(egui_ctx.ctx_mut(), |ui| {
        egui::warn_if_debug_build(ui);

        match main_ui_state.view {
            Tabs::Transcoder => {
                transcoder_state.central_panel_ui(ui, time);
            }
            Tabs::Player => {
                player_state.central_panel_ui(ui, time);
            }
        }

        /*
        Images in the central panel are common to both visualization tabs, so we can do this
         here as the last step of drawing its UI
        */
        let mut has_input = false;
        let avail_size = ui.available_size();
        ui.horizontal(|ui| {
            ui.set_max_size(avail_size);

            ui.vertical(|ui| {
                if let (Some(input), Some(input_texture_id)) = (input, input_texture_id) {
                    let mut avail_size = ui.available_size();
                    avail_size.x = avail_size.x / 2.0 - ui.spacing().item_spacing.y / 2.0;

                    ui.set_max_size(avail_size);

                    // Right-align the text so it's easier to compare to the ADDER version
                    ui.with_layout(egui::Layout::top_down(Align::Max), |ui| {
                        let mut job = LayoutJob::default();
                        job.append(
                            "Input\n",
                            0.0,
                            TextFormat {
                                font_id: FontId::new(24.0, FontFamily::Proportional),
                                color: Color32::WHITE,
                                ..Default::default()
                            },
                        );

                        let last = transcoder_state
                            .ui_info_state
                            .plot_points_raw_source_bitrate_y
                            .points
                            .iter()
                            .last();
                        let str_num = match last {
                            None => -999.0,
                            Some(item) => item.unwrap_or(-999.0),
                        };

                        let str = format!("{number:.prec$} MB/s", prec = 2, number = str_num);
                        job.append(
                            &str,
                            0.0,
                            TextFormat {
                                font_id: FontId::new(14.0, FontFamily::Proportional),
                                ..Default::default()
                            },
                        );
                        ui.label(job);
                    });
                    has_input = true;

                    let size = match (
                        input.texture_descriptor.size.width as f32,
                        input.texture_descriptor.size.height as f32,
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
                    ui.image(input_texture_id, size);
                }
            });

            ui.vertical(|ui| {
                if let (Some(image), Some(texture_id)) = (image, texture_id) {
                    ui.with_layout(egui::Layout::top_down(Align::Min), |ui| {
                        let mut job = LayoutJob::default();
                        job.append(
                            "ADΔER\n",
                            0.0,
                            TextFormat {
                                font_id: FontId::new(24.0, FontFamily::Proportional),
                                color: Color32::WHITE,
                                ..Default::default()
                            },
                        );

                        let (bitrate, percentage_str, color) = match main_ui_state.view {
                            Tabs::Transcoder => {
                                let last = transcoder_state
                                    .ui_info_state
                                    .plot_points_raw_adder_bitrate_y
                                    .points
                                    .iter()
                                    .last();
                                let adder_bitrate = match last {
                                    None => -999.0,
                                    Some(item) => item.unwrap_or(-999.0),
                                };

                                let last = transcoder_state
                                    .ui_info_state
                                    .plot_points_raw_source_bitrate_y
                                    .points
                                    .iter()
                                    .last();
                                let source_bitrate = match last {
                                    None => -999.0,
                                    Some(item) => item.unwrap_or(-999.0),
                                };

                                let percentage = adder_bitrate / source_bitrate * 100.0;

                                let percentage_str =
                                    format!("{number:.prec$}%", prec = 2, number = percentage);
                                let color = if percentage < 100.0 {
                                    Color32::GREEN
                                } else {
                                    Color32::RED
                                };
                                (adder_bitrate, percentage_str, color)
                            }
                            Tabs::Player => {
                                let last = transcoder_state
                                    .ui_info_state
                                    .plot_points_raw_adder_bitrate_y
                                    .points
                                    .iter()
                                    .last();
                                let adder_bitrate = match last {
                                    None => -999.0,
                                    Some(item) => item.unwrap_or(-999.0),
                                };
                                (adder_bitrate, "".to_string(), Color32::WHITE)
                            }
                        };

                        let str = format!("{number:.prec$} MB/s | ", prec = 2, number = bitrate);
                        job.append(
                            &str,
                            0.0,
                            TextFormat {
                                font_id: FontId::new(14.0, FontFamily::Proportional),
                                ..Default::default()
                            },
                        );

                        job.append(
                            &percentage_str,
                            0.0,
                            TextFormat {
                                font_id: FontId::new(14.0, FontFamily::Proportional),
                                color,
                                ..Default::default()
                            },
                        );

                        ui.label(job);
                    });

                    let avail_size = ui.available_size();
                    let size = match (
                        image.texture_descriptor.size.width as f32,
                        image.texture_descriptor.size.height as f32,
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

                    ui.image(texture_id, size);
                }
            });
        });

        if let Some(msg) = main_ui_state.error_msg.as_ref() {
            ui.label(msg);
        }
    });
}

fn update_adder_params(
    main_ui_state: Res<MainUiState>,
    handles: Res<Images>,
    images: ResMut<Assets<Image>>,
    mut transcoder_state: ResMut<TranscoderState>,
) {
    match main_ui_state.view {
        Tabs::Transcoder => {
            transcoder_state.update_adder_params(handles, images);
        }
        Tabs::Player => {
            // player_state.update_adder_params(commands);
        }
    }
}

fn consume_source(
    images: ResMut<Assets<Image>>,
    handles: ResMut<Images>,
    mut main_ui_state: ResMut<MainUiState>,
    mut transcoder_state: ResMut<TranscoderState>,
    mut player_state: ResMut<PlayerState>,
) {
    let res = match main_ui_state.view {
        Tabs::Transcoder => transcoder_state.consume_source(images, handles),
        Tabs::Player => player_state.consume_source(images, handles),
    };

    match res {
        Ok(_) => {}
        Err(e) => {
            if e.is::<std::sync::mpsc::TryRecvError>() {
                main_ui_state.error_msg = Some("Loading file...".to_string());
            } else {
                main_ui_state.error_msg = Some(format!("{e}"));
            }
        }
    }
}

#[derive(Component, Default)]
struct MyDropTarget;

///<https://bevy-cheatbook.github.io/input/dnd.html>
fn file_drop(
    main_ui_state: ResMut<MainUiState>,
    mut player_state: ResMut<PlayerState>,
    mut transcoder_state: ResMut<TranscoderState>,
    mut dnd_evr: EventReader<FileDragAndDrop>,
    query_ui_droptarget: Query<&Interaction, With<MyDropTarget>>,
) {
    for ev in dnd_evr.iter() {
        if let FileDragAndDrop::DroppedFile { path_buf, .. } = ev {
            for interaction in query_ui_droptarget.iter() {
                if *interaction == Interaction::Hovered {
                    // it was dropped over our UI element
                    // (our UI element is being hovered over)
                }
            }

            match main_ui_state.view {
                Tabs::Transcoder => {
                    transcoder_state.ui_info_state.input_path_0 = Some(path_buf.clone());
                    transcoder_state.ui_info_state.input_path_1 = None;

                    let output_path_opt = transcoder_state.ui_info_state.output_path.clone();
                    // TODO: refactor as struct func
                    replace_adder_transcoder(
                        &mut transcoder_state,
                        Some(path_buf.clone()),
                        None,
                        output_path_opt,
                        0,
                    ); // TODO!!
                }
                Tabs::Player => {
                    player_state.replace_player(path_buf);
                    player_state.play();
                }
            }
        }
    }
}

/// A slider with +/- buttons. Returns true if the value was changed.
fn slider_pm<Num: emath::Numeric + Pm>(
    enabled: bool,
    logarithmic: bool,
    ui: &mut Ui,
    instant_value: &mut Num,
    drag_value: &mut Num,
    range: RangeInclusive<Num>,
    notches: Vec<Num>,
    interval: Num,
) -> bool {
    let start_value = *instant_value;
    ui.add_enabled_ui(enabled, |ui| {
        ui.horizontal(|ui| {
            if ui.button("-").clicked() {
                instant_value.decrement(range.start(), &interval);
                *drag_value = *instant_value;
            }
            let slider = ui.add(
                NotchedSlider::new(drag_value, range.clone(), notches).logarithmic(logarithmic),
            );
            if slider.drag_released() {
                *instant_value = *drag_value;
            }
            if slider.lost_focus() {
                *instant_value = *drag_value;
            }

            if ui.button("+").clicked() {
                instant_value.increment(range.end(), &interval);
                *drag_value = *instant_value;
            }
        });
    });

    *instant_value != start_value
}

fn add_slider_row<Num: emath::Numeric + Pm>(
    enabled: bool,
    logarithmic: bool,
    label: impl Into<WidgetText>,
    ui: &mut Ui,
    instant_value: &mut Num,
    drag_value: &mut Num,
    range: RangeInclusive<Num>,
    notches: Vec<Num>,
    interval: Num,
) -> bool {
    ui.add_enabled(enabled, egui::Label::new(label));
    let ret = slider_pm(
        enabled,
        logarithmic,
        ui,
        instant_value,
        drag_value,
        range,
        notches,
        interval,
    );
    ui.end_row();
    ret
}

fn add_checkbox_row(
    enabled: bool,
    label_1: impl Into<WidgetText>,
    label_2: impl Into<WidgetText>,
    ui: &mut Ui,
    checkbox_value: &mut bool,
) -> bool {
    ui.add_enabled(enabled, egui::Label::new(label_1));
    let ret = ui
        .add_enabled(enabled, egui::Checkbox::new(checkbox_value, label_2))
        .changed();
    ui.end_row();
    ret
}

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
