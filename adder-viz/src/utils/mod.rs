use crate::utils::slider::NotchedSlider;
use crate::Pm;
use eframe::emath;
use eframe::epaint::ColorImage;
use egui::{Ui, WidgetText};
use egui_plot::{Line, PlotPoints};
use ndarray::Axis;
use rayon::iter::IntoParallelIterator;
use rayon::iter::ParallelIterator;
use std::collections::VecDeque;
use std::error::Error;
use std::ops::RangeInclusive;
use video_rs_adder_dep::Frame;
pub(crate) mod slider;

pub(crate) struct PlotY {
    pub points: VecDeque<Option<f64>>,
}

impl PlotY {
    pub(crate) fn get_plotline(&self, name: &str, log_base: bool) -> Line {
        let plot_points: PlotPoints = (0..1000)
            .map(|i| {
                let x = i as f64;
                let y = self.points[i].unwrap_or(0.0);
                if log_base && y > 0.0 {
                    [x, y.log10()]
                } else {
                    [x, y]
                }
            })
            .collect();
        Line::new(plot_points).name(name)
    }

    pub(crate) fn update(&mut self, new_opt: Option<f64>) {
        match new_opt {
            Some(new) => {
                if new.is_finite() {
                    self.points.push_back(Some(new));
                } else {
                    self.points.push_back(Some(0.0));
                }
            }
            None => self.points.push_back(None),
        }
        self.points.pop_front();
    }
}

#[inline]
pub fn prep_epaint_image(
    image_mat: &mut Frame,
    color: bool,
    width: usize,
    height: usize,
) -> Result<ColorImage, Box<dyn Error>> {
    if color {
        image_mat
            .axis_iter_mut(Axis(0))
            .into_par_iter()
            .for_each(|mut slice| {
                slice.axis_iter_mut(Axis(0)).for_each(|mut row| {
                    let y = row[0] as f32;
                    let u = row[1] as f32 - 128.0;
                    let v = row[2] as f32 - 128.0;
                    row[0] = (y + 1.402 * v).clamp(0.0, 255.0) as u8; // R
                    row[1] = (y - 0.344136 * u - 0.714136 * v).clamp(0.0, 255.0) as u8; // G
                    row[2] = (y + 1.772 * u).clamp(0.0, 255.0) as u8; // B
                });
            });

        return Ok(ColorImage::from_rgb(
            [width, height],
            image_mat.as_standard_layout().as_slice().unwrap(),
        ));
    } else {
        return Ok(ColorImage::from_gray(
            [width, height],
            image_mat.as_standard_layout().as_slice().unwrap(),
        ));
    }
}

pub fn add_checkbox_row(
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

pub fn add_slider_row<Num: emath::Numeric + Pm>(
    enabled: bool,
    logarithmic: bool,
    label: impl Into<WidgetText>,
    ui: &mut Ui,
    instant_value: &mut Num,
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
        range,
        notches,
        interval,
    );
    ui.end_row();
    ret
}

/// A slider with +/- buttons. Returns true if the value was changed.
pub fn slider_pm<Num: egui::emath::Numeric + Pm>(
    enabled: bool,
    logarithmic: bool,
    ui: &mut egui::Ui,
    value: &mut Num,
    range: RangeInclusive<Num>,
    notches: Vec<Num>,
    interval: Num,
) -> bool {
    let start_value = *value;
    let mut button_down = false;
    ui.add_enabled_ui(enabled, |ui| {
        ui.horizontal(|ui| {
            if ui.button("-").clicked() {
                value.decrement(range.start(), &interval);
            }

            let response =
                ui.add(NotchedSlider::new(value, range.clone(), notches).logarithmic(logarithmic));

            if response.is_pointer_button_down_on() {
                button_down = true;
            }

            if ui.button("+").clicked() {
                value.increment(range.end(), &interval);
            }
        });
    });

    button_down
}
