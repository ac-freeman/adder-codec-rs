mod player;
mod transcoder;
mod utils;

use crate::player::ui::PlayerUi;
use crate::transcoder::ui::TranscoderUi;
use eframe::egui;
use egui::{Ui, Widget, WidgetText};
use strum::IntoEnumIterator;
use strum_macros::EnumIter;

fn main() {
    let native_options = eframe::NativeOptions::default();
    if let Err(e) = eframe::run_native(
        "ADΔER Viz",
        native_options,
        Box::new(|cc| Box::new(App::new(cc))),
    ) {
        eprintln!("Error running ADΔER Viz: {e}");
    }
}

struct App {
    view: Tabs,
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
            transcoder_ui: TranscoderUi::new(cc),
            player_ui: PlayerUi::new(cc),
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
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
