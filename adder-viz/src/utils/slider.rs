/*
This was manually copied from egui/src/widgets/slider.rs. I modified it to add "notches" (divisors)
to the slider. This will be removed in the future, if egui adds an equivalent feature.
 */

use egui::{
    emath, epaint, lerp, pos2, remap, remap_clamp, vec2, Color32, DragValue, Key, Label, NumExt,
    Pos2, Rect, Response, Sense, SliderOrientation, Stroke, TextStyle, Ui, Vec2, Widget,
    WidgetInfo, WidgetText,
};
use std::ops::RangeInclusive;

type NumFormatter<'a> = Box<dyn 'a + Fn(f64, RangeInclusive<usize>) -> String>;
type NumParser<'a> = Box<dyn 'a + Fn(&str) -> Option<f64>>;

// ----------------------------------------------------------------------------

/// Combined into one function (rather than two) to make it easier
/// for the borrow checker.
type GetSetValue<'a> = Box<dyn 'a + FnMut(Option<f64>) -> f64>;

fn get(get_set_value: &mut GetSetValue<'_>) -> f64 {
    (get_set_value)(None)
}

fn set(get_set_value: &mut GetSetValue<'_>, value: f64) {
    (get_set_value)(Some(value));
}

// ----------------------------------------------------------------------------

#[derive(Clone)]
#[allow(dead_code)]
struct SliderSpec {
    logarithmic: bool,

    /// For logarithmic sliders, the smallest positive value we are interested in.
    /// 1 for integer sliders, maybe 1e-6 for others.
    smallest_positive: f64,

    /// For logarithmic sliders, the largest positive value we are interested in
    /// before the slider switches to `INFINITY`, if that is the higher end.
    /// Default: INFINITY.
    largest_finite: f64,
}

#[must_use = "You should put this widget in an ui with `ui.add(widget);`"]
#[allow(dead_code)]
pub struct NotchedSlider<'a> {
    get_set_value: GetSetValue<'a>,
    range: RangeInclusive<f64>,
    notches: Vec<f64>,
    spec: SliderSpec,
    clamp_to_range: bool,
    smart_aim: bool,
    show_value: bool,
    orientation: SliderOrientation,
    prefix: String,
    suffix: String,
    text: WidgetText,
    /// Sets the minimal step of the widget value
    step: Option<f64>,
    min_decimals: usize,
    max_decimals: Option<usize>,
    custom_formatter: Option<NumFormatter<'a>>,
    custom_parser: Option<NumParser<'a>>,
}

#[allow(dead_code)]
impl<'a> NotchedSlider<'a> {
    /// Creates a new horizontal slider.
    pub fn new<Num: emath::Numeric>(
        value: &'a mut Num,
        range: RangeInclusive<Num>,
        notches: Vec<Num>,
    ) -> Self {
        let range_f64 = range.start().to_f64()..=range.end().to_f64();
        let notches_f64 = notches.into_iter().map(|n| n.to_f64()).collect();
        let slf = Self::from_get_set(range_f64, notches_f64, move |v: Option<f64>| {
            if let Some(v) = v {
                *value = Num::from_f64(v);
            }
            value.to_f64()
        });

        if Num::INTEGRAL {
            slf.integer()
        } else {
            slf
        }
    }

    pub fn from_get_set(
        range: RangeInclusive<f64>,
        notches: Vec<f64>,
        get_set_value: impl 'a + FnMut(Option<f64>) -> f64,
    ) -> Self {
        Self {
            get_set_value: Box::new(get_set_value),
            range,
            notches,
            spec: SliderSpec {
                logarithmic: false,
                smallest_positive: 1e-6,
                largest_finite: f64::INFINITY,
            },
            clamp_to_range: true,
            smart_aim: true,
            show_value: true,
            orientation: SliderOrientation::Horizontal,
            prefix: Default::default(),
            suffix: Default::default(),
            text: Default::default(),
            step: None,
            min_decimals: 0,
            max_decimals: None,
            custom_formatter: None,
            custom_parser: None,
        }
    }

    /// Control whether or not the slider shows the current value.
    /// Default: `true`.
    pub fn show_value(mut self, show_value: bool) -> Self {
        self.show_value = show_value;
        self
    }

    /// Show a prefix before the number, e.g. "x: "
    pub fn prefix(mut self, prefix: impl ToString) -> Self {
        self.prefix = prefix.to_string();
        self
    }

    /// Add a suffix to the number, this can be e.g. a unit ("°" or " m")
    pub fn suffix(mut self, suffix: impl ToString) -> Self {
        self.suffix = suffix.to_string();
        self
    }

    /// Show a text next to the slider (e.g. explaining what the slider controls).
    pub fn text(mut self, text: impl Into<WidgetText>) -> Self {
        self.text = text.into();
        self
    }

    pub fn text_color(mut self, text_color: Color32) -> Self {
        self.text = self.text.color(text_color);
        self
    }

    /// Vertical or horizontal slider? The default is horizontal.
    pub fn orientation(mut self, orientation: SliderOrientation) -> Self {
        self.orientation = orientation;
        self
    }

    /// Make this a vertical slider.
    pub fn vertical(mut self) -> Self {
        self.orientation = SliderOrientation::Vertical;
        self
    }

    /// Make this a logarithmic slider.
    /// This is great for when the slider spans a huge range,
    /// e.g. from one to a million.
    /// The default is OFF.
    pub fn logarithmic(mut self, logarithmic: bool) -> Self {
        self.spec.logarithmic = logarithmic;
        self
    }

    /// For logarithmic sliders that includes zero:
    /// what is the smallest positive value you want to be able to select?
    /// The default is `1` for integer sliders and `1e-6` for real sliders.
    pub fn smallest_positive(mut self, smallest_positive: f64) -> Self {
        self.spec.smallest_positive = smallest_positive;
        self
    }

    /// For logarithmic sliders, the largest positive value we are interested in
    /// before the slider switches to `INFINITY`, if that is the higher end.
    /// Default: INFINITY.
    pub fn largest_finite(mut self, largest_finite: f64) -> Self {
        self.spec.largest_finite = largest_finite;
        self
    }

    /// If set to `true`, all incoming and outgoing values will be clamped to the slider range.
    /// Default: `true`.
    pub fn clamp_to_range(mut self, clamp_to_range: bool) -> Self {
        self.clamp_to_range = clamp_to_range;
        self
    }

    /// Turn smart aim on/off. Default is ON.
    /// There is almost no point in turning this off.
    pub fn smart_aim(mut self, smart_aim: bool) -> Self {
        self.smart_aim = smart_aim;
        self
    }

    /// Sets the minimal change of the value.
    /// Value `0.0` effectively disables the feature. If the new value is out of range
    /// and `clamp_to_range` is enabled, you would not have the ability to change the value.
    ///
    /// Default: `0.0` (disabled).
    pub fn step_by(mut self, step: f64) -> Self {
        self.step = if step != 0.0 { Some(step) } else { None };
        self
    }

    // TODO(emilk): we should also have a "min precision".
    /// Set a minimum number of decimals to display.
    /// Normally you don't need to pick a precision, as the slider will intelligently pick a precision for you.
    /// Regardless of precision the slider will use "smart aim" to help the user select nice, round values.
    pub fn min_decimals(mut self, min_decimals: usize) -> Self {
        self.min_decimals = min_decimals;
        self
    }

    // TODO(emilk): we should also have a "max precision".
    /// Set a maximum number of decimals to display.
    /// Values will also be rounded to this number of decimals.
    /// Normally you don't need to pick a precision, as the slider will intelligently pick a precision for you.
    /// Regardless of precision the slider will use "smart aim" to help the user select nice, round values.
    pub fn max_decimals(mut self, max_decimals: usize) -> Self {
        self.max_decimals = Some(max_decimals);
        self
    }

    /// Set an exact number of decimals to display.
    /// Values will also be rounded to this number of decimals.
    /// Normally you don't need to pick a precision, as the slider will intelligently pick a precision for you.
    /// Regardless of precision the slider will use "smart aim" to help the user select nice, round values.
    pub fn fixed_decimals(mut self, num_decimals: usize) -> Self {
        self.min_decimals = num_decimals;
        self.max_decimals = Some(num_decimals);
        self
    }

    /// Set custom formatter defining how numbers are converted into text.
    ///
    /// A custom formatter takes a `f64` for the numeric value and a `RangeInclusive<usize>` representing
    /// the decimal range i.e. minimum and maximum number of decimal places shown.
    ///
    /// See also: [`DragValue::custom_parser`]
    ///
    /// ```
    /// # egui::__run_test_ui(|ui| {
    /// # let mut my_i32: i32 = 0;
    /// ui.add(egui::Slider::new(&mut my_i32, 0..=((60 * 60 * 24) - 1))
    ///     .custom_formatter(|n, _| {
    ///         let n = n as i32;
    ///         let hours = n / (60 * 60);
    ///         let mins = (n / 60) % 60;
    ///         let secs = n % 60;
    ///         format!("{hours:02}:{mins:02}:{secs:02}")
    ///     })
    ///     .custom_parser(|s| {
    ///         let parts: Vec<&str> = s.split(':').collect();
    ///         if parts.len() == 3 {
    ///             parts[0].parse::<i32>().and_then(|h| {
    ///                 parts[1].parse::<i32>().and_then(|m| {
    ///                     parts[2].parse::<i32>().map(|s| {
    ///                         ((h * 60 * 60) + (m * 60) + s) as f64
    ///                     })
    ///                 })
    ///             })
    ///             .ok()
    ///         } else {
    ///             None
    ///         }
    ///     }));
    /// # });
    /// ```
    pub fn custom_formatter(
        mut self,
        formatter: impl 'a + Fn(f64, RangeInclusive<usize>) -> String,
    ) -> Self {
        self.custom_formatter = Some(Box::new(formatter));
        self
    }

    /// Set custom parser defining how the text input is parsed into a number.
    ///
    /// A custom parser takes an `&str` to parse into a number and returns `Some` if it was successfully parsed
    /// or `None` otherwise.
    ///
    /// See also: [`DragValue::custom_formatter`]
    ///
    /// ```
    /// # egui::__run_test_ui(|ui| {
    /// # let mut my_i32: i32 = 0;
    /// ui.add(egui::Slider::new(&mut my_i32, 0..=((60 * 60 * 24) - 1))
    ///     .custom_formatter(|n, _| {
    ///         let n = n as i32;
    ///         let hours = n / (60 * 60);
    ///         let mins = (n / 60) % 60;
    ///         let secs = n % 60;
    ///         format!("{hours:02}:{mins:02}:{secs:02}")
    ///     })
    ///     .custom_parser(|s| {
    ///         let parts: Vec<&str> = s.split(':').collect();
    ///         if parts.len() == 3 {
    ///             parts[0].parse::<i32>().and_then(|h| {
    ///                 parts[1].parse::<i32>().and_then(|m| {
    ///                     parts[2].parse::<i32>().map(|s| {
    ///                         ((h * 60 * 60) + (m * 60) + s) as f64
    ///                     })
    ///                 })
    ///             })
    ///             .ok()
    ///         } else {
    ///             None
    ///         }
    ///     }));
    /// # });
    /// ```
    pub fn custom_parser(mut self, parser: impl 'a + Fn(&str) -> Option<f64>) -> Self {
        self.custom_parser = Some(Box::new(parser));
        self
    }

    /// Set `custom_formatter` and `custom_parser` to display and parse numbers as binary integers. Floating point
    /// numbers are *not* supported.
    ///
    /// `min_width` specifies the minimum number of displayed digits; if the number is shorter than this, it will be
    /// prefixed with additional 0s to match `min_width`.
    ///
    /// If `twos_complement` is true, negative values will be displayed as the 2's complement representation. Otherwise
    /// they will be prefixed with a '-' sign.
    ///
    /// # Panics
    ///
    /// Panics if `min_width` is 0.
    ///
    /// ```
    /// # egui::__run_test_ui(|ui| {
    /// # let mut my_i32: i32 = 0;
    /// ui.add(egui::Slider::new(&mut my_i32, -100..=100).binary(64, false));
    /// # });
    /// ```
    pub fn binary(self, min_width: usize, twos_complement: bool) -> Self {
        assert!(
            min_width > 0,
            "Slider::binary: `min_width` must be greater than 0"
        );
        if twos_complement {
            self.custom_formatter(move |n, _| format!("{:0>min_width$b}", n as i64))
        } else {
            self.custom_formatter(move |n, _| {
                let sign = if n < 0.0 { "-" } else { "" };
                format!("{sign}{:0>min_width$b}", n.abs() as i64)
            })
        }
        .custom_parser(|s| i64::from_str_radix(s, 2).map(|n| n as f64).ok())
    }

    /// Set `custom_formatter` and `custom_parser` to display and parse numbers as octal integers. Floating point
    /// numbers are *not* supported.
    ///
    /// `min_width` specifies the minimum number of displayed digits; if the number is shorter than this, it will be
    /// prefixed with additional 0s to match `min_width`.
    ///
    /// If `twos_complement` is true, negative values will be displayed as the 2's complement representation. Otherwise
    /// they will be prefixed with a '-' sign.
    ///
    /// # Panics
    ///
    /// Panics if `min_width` is 0.
    ///
    /// ```
    /// # egui::__run_test_ui(|ui| {
    /// # let mut my_i32: i32 = 0;
    /// ui.add(egui::Slider::new(&mut my_i32, -100..=100).octal(22, false));
    /// # });
    /// ```
    pub fn octal(self, min_width: usize, twos_complement: bool) -> Self {
        assert!(
            min_width > 0,
            "Slider::octal: `min_width` must be greater than 0"
        );
        if twos_complement {
            self.custom_formatter(move |n, _| format!("{:0>min_width$o}", n as i64))
        } else {
            self.custom_formatter(move |n, _| {
                let sign = if n < 0.0 { "-" } else { "" };
                format!("{sign}{:0>min_width$o}", n.abs() as i64)
            })
        }
        .custom_parser(|s| i64::from_str_radix(s, 8).map(|n| n as f64).ok())
    }

    /// Set `custom_formatter` and `custom_parser` to display and parse numbers as hexadecimal integers. Floating point
    /// numbers are *not* supported.
    ///
    /// `min_width` specifies the minimum number of displayed digits; if the number is shorter than this, it will be
    /// prefixed with additional 0s to match `min_width`.
    ///
    /// If `twos_complement` is true, negative values will be displayed as the 2's complement representation. Otherwise
    /// they will be prefixed with a '-' sign.
    ///
    /// # Panics
    ///
    /// Panics if `min_width` is 0.
    ///
    /// ```
    /// # egui::__run_test_ui(|ui| {
    /// # let mut my_i32: i32 = 0;
    /// ui.add(egui::Slider::new(&mut my_i32, -100..=100).hexadecimal(16, false, true));
    /// # });
    /// ```
    pub fn hexadecimal(self, min_width: usize, twos_complement: bool, upper: bool) -> Self {
        assert!(
            min_width > 0,
            "Slider::hexadecimal: `min_width` must be greater than 0"
        );
        match (twos_complement, upper) {
            (true, true) => {
                self.custom_formatter(move |n, _| format!("{:0>min_width$X}", n as i64))
            }
            (true, false) => {
                self.custom_formatter(move |n, _| format!("{:0>min_width$x}", n as i64))
            }
            (false, true) => self.custom_formatter(move |n, _| {
                let sign = if n < 0.0 { "-" } else { "" };
                format!("{sign}{:0>min_width$X}", n.abs() as i64)
            }),
            (false, false) => self.custom_formatter(move |n, _| {
                let sign = if n < 0.0 { "-" } else { "" };
                format!("{sign}{:0>min_width$x}", n.abs() as i64)
            }),
        }
        .custom_parser(|s| i64::from_str_radix(s, 16).map(|n| n as f64).ok())
    }

    /// Helper: equivalent to `self.precision(0).smallest_positive(1.0)`.
    /// If you use one of the integer constructors (e.g. `Slider::i32`) this is called for you,
    /// but if you want to have a slider for picking integer values in an `Slider::f64`, use this.
    pub fn integer(self) -> Self {
        self.fixed_decimals(0).smallest_positive(1.0).step_by(1.0)
    }

    fn get_value(&mut self) -> f64 {
        let value = get(&mut self.get_set_value);
        if self.clamp_to_range {
            let start = *self.range.start();
            let end = *self.range.end();
            value.clamp(start.min(end), start.max(end))
        } else {
            value
        }
    }

    fn set_value(&mut self, mut value: f64) {
        if self.clamp_to_range {
            let start = *self.range.start();
            let end = *self.range.end();
            value = value.clamp(start.min(end), start.max(end));
        }
        if let Some(max_decimals) = self.max_decimals {
            value = emath::round_to_decimals(value, max_decimals);
        }
        if let Some(step) = self.step {
            value = (value / step).round() * step;
        }
        set(&mut self.get_set_value, value);
    }

    fn clamp_range(&self) -> RangeInclusive<f64> {
        if self.clamp_to_range {
            self.range()
        } else {
            f64::NEG_INFINITY..=f64::INFINITY
        }
    }

    fn range(&self) -> RangeInclusive<f64> {
        self.range.clone()
    }

    /// For instance, `position` is the mouse position and `position_range` is the physical location of the slider on the screen.
    fn value_from_position(&self, position: f32, position_range: RangeInclusive<f32>) -> f64 {
        let normalized = remap_clamp(position, position_range, 0.0..=1.0) as f64;
        value_from_normalized(normalized, self.range(), &self.spec)
    }

    fn position_from_value(&self, value: f64, position_range: RangeInclusive<f32>) -> f32 {
        let normalized = normalized_from_value(value, self.range(), &self.spec);
        lerp(position_range, normalized as f32)
    }
}

impl<'a> NotchedSlider<'a> {
    /// Just the slider, no text
    fn allocate_slider_space(&self, ui: &mut Ui, thickness: f32) -> Response {
        let desired_size = match self.orientation {
            SliderOrientation::Horizontal => vec2(ui.spacing().slider_width, thickness),
            SliderOrientation::Vertical => vec2(thickness, ui.spacing().slider_width),
        };
        ui.allocate_response(desired_size, Sense::drag())
    }

    /// Just the slider, no text
    fn slider_ui(&mut self, ui: &mut Ui, response: &Response) {
        let rect = &response.rect;
        let position_range = self.position_range(rect);

        if let Some(pointer_position_2d) = response.interact_pointer_pos() {
            let position = self.pointer_position(pointer_position_2d);
            let new_value = if self.smart_aim {
                let aim_radius = ui.input(|i| i.aim_radius());
                let value = emath::smart_aim::best_in_range_f64(
                    self.value_from_position(position - aim_radius, position_range.clone()),
                    self.value_from_position(position + aim_radius, position_range.clone()),
                );
                if self.notches.is_empty() {
                    value
                } else {
                    // let snap_radius = (self.range.end() - self.range.start()) / 10.0;
                    let snap_radius = 0.1;
                    // Snap to closest notch
                    let mut closest_notch = 0.0;
                    let norm_value = normalized_from_value(value, self.range.clone(), &self.spec);
                    let mut closest_distance = (norm_value - closest_notch).abs();
                    for notch in self.notches.iter().copied().chain(Some(*self.range.end())) {
                        let notch_norm_value =
                            normalized_from_value(notch, self.range.clone(), &self.spec);
                        let distance = (norm_value - notch_norm_value).abs();
                        if distance < closest_distance {
                            closest_notch = notch;
                            closest_distance = distance;
                        }
                    }
                    if closest_distance < snap_radius {
                        closest_notch
                    } else {
                        value
                    }
                }
            } else {
                self.value_from_position(position, position_range.clone())
            };
            self.set_value(new_value);
        }

        let mut decrement = 0usize;
        let mut increment = 0usize;

        if response.has_focus() {
            let (dec_key, inc_key) = match self.orientation {
                SliderOrientation::Horizontal => (Key::ArrowLeft, Key::ArrowRight),
                // Note that this is for moving the slider position,
                // so up = decrement y coordinate:
                SliderOrientation::Vertical => (Key::ArrowUp, Key::ArrowDown),
            };

            decrement += ui.input(|i| i.num_presses(dec_key));
            increment += ui.input(|i| i.num_presses(inc_key));
        }

        #[cfg(feature = "accesskit")]
        {
            use accesskit::Action;
            decrement += ui
                .input()
                .num_accesskit_action_requests(response.id, Action::Decrement);
            increment += ui
                .input()
                .num_accesskit_action_requests(response.id, Action::Increment);
        }

        let kb_step = increment as f32 - decrement as f32;

        if kb_step != 0.0 {
            let prev_value = self.get_value();
            let prev_position = self.position_from_value(prev_value, position_range.clone());
            let new_position = prev_position + kb_step;
            let new_value = match self.step {
                Some(step) => prev_value + (kb_step as f64 * step),
                None if self.smart_aim => {
                    let aim_radius = ui.input(|i| i.aim_radius());
                    emath::smart_aim::best_in_range_f64(
                        self.value_from_position(new_position - aim_radius, position_range.clone()),
                        self.value_from_position(new_position + aim_radius, position_range.clone()),
                    )
                }
                _ => self.value_from_position(new_position, position_range.clone()),
            };
            self.set_value(new_value);
        }

        #[cfg(feature = "accesskit")]
        {
            use accesskit::{Action, ActionData};
            for request in ui
                .input()
                .accesskit_action_requests(response.id, Action::SetValue)
            {
                if let Some(ActionData::NumericValue(new_value)) = request.data {
                    self.set_value(new_value);
                }
            }
        }

        // Paint it:
        if ui.is_rect_visible(response.rect) {
            let value = self.get_value();

            let rail_radius = ui.painter().round_to_pixel(self.rail_radius_limit(rect));
            let mut rail_rect = self.rail_rect(rect, rail_radius);
            rail_rect = rail_rect.shrink2(Vec2 { x: 0.0, y: 1.5 });

            let position_1d = self.position_from_value(value, position_range.clone());

            let visuals = ui.style().interact(response);
            ui.painter().add(epaint::RectShape {
                rect: rail_rect,
                rounding: ui.visuals().widgets.inactive.rounding,
                fill: ui.visuals().widgets.inactive.bg_fill,
                // fill: visuals.bg_fill,
                // fill: ui.visuals().extreme_bg_color,
                stroke: Stroke {
                    width: 0.05,
                    color: Default::default(),
                },
                // stroke: visuals.bg_stroke,
                // stroke: ui.visuals().widgets.inactive.bg_stroke,
                fill_texture_id: Default::default(),
                uv: Rect {
                    min: Default::default(),
                    max: Default::default(),
                },
            });

            for notch in &self.notches {
                let notch_position_1d = self.position_from_value(*notch, position_range.clone());
                let center = self.marker_center(notch_position_1d, &rail_rect);
                ui.painter().add(epaint::CircleShape {
                    center,
                    radius: 2.0,
                    fill: ui.visuals().widgets.noninteractive.bg_fill,
                    stroke: Stroke {
                        width: 0.0,
                        color: Default::default(),
                    },
                });
            }

            let center = self.marker_center(position_1d, &rail_rect);

            ui.painter().add(epaint::CircleShape {
                center,
                radius: self.handle_radius(rect) + visuals.expansion,
                fill: visuals.bg_fill,
                stroke: visuals.fg_stroke,
            });
        }
    }

    fn marker_center(&self, position_1d: f32, rail_rect: &Rect) -> Pos2 {
        match self.orientation {
            SliderOrientation::Horizontal => pos2(position_1d, rail_rect.center().y),
            SliderOrientation::Vertical => pos2(rail_rect.center().x, position_1d),
        }
    }

    fn pointer_position(&self, pointer_position_2d: Pos2) -> f32 {
        match self.orientation {
            SliderOrientation::Horizontal => pointer_position_2d.x,
            SliderOrientation::Vertical => pointer_position_2d.y,
        }
    }

    fn position_range(&self, rect: &Rect) -> RangeInclusive<f32> {
        let handle_radius = self.handle_radius(rect);
        match self.orientation {
            SliderOrientation::Horizontal => {
                (rect.left() + handle_radius)..=(rect.right() - handle_radius)
            }
            SliderOrientation::Vertical => {
                (rect.bottom() - handle_radius)..=(rect.top() + handle_radius)
            }
        }
    }

    fn rail_rect(&self, rect: &Rect, radius: f32) -> Rect {
        match self.orientation {
            SliderOrientation::Horizontal => Rect::from_min_max(
                pos2(rect.left(), rect.center().y - radius),
                pos2(rect.right(), rect.center().y + radius),
            ),
            SliderOrientation::Vertical => Rect::from_min_max(
                pos2(rect.center().x - radius, rect.top()),
                pos2(rect.center().x + radius, rect.bottom()),
            ),
        }
    }

    fn handle_radius(&self, rect: &Rect) -> f32 {
        let limit = match self.orientation {
            SliderOrientation::Horizontal => rect.height(),
            SliderOrientation::Vertical => rect.width(),
        };
        limit / 2.5
    }

    fn rail_radius_limit(&self, rect: &Rect) -> f32 {
        match self.orientation {
            SliderOrientation::Horizontal => (rect.height() / 4.0).at_least(2.0),
            SliderOrientation::Vertical => (rect.width() / 4.0).at_least(2.0),
        }
    }

    fn value_ui(&mut self, ui: &mut Ui, position_range: RangeInclusive<f32>) -> Response {
        // If [`DragValue`] is controlled from the keyboard and `step` is defined, set speed to `step`
        let change = {
            // Hold one lock rather than 4 (see https://github.com/emilk/egui/pull/1380).
            ui.input(|i| {
                i.num_presses(Key::ArrowUp) as i32 + i.num_presses(Key::ArrowRight) as i32
                    - i.num_presses(Key::ArrowDown) as i32
                    - i.num_presses(Key::ArrowLeft) as i32
            })
        };
        let speed = match self.step {
            Some(step) if change != 0 => step,
            _ => self.current_gradient(&position_range),
        };
        let mut value = self.get_value();
        let response = ui.add({
            let mut dv = DragValue::new(&mut value)
                .speed(speed)
                .clamp_range(self.clamp_range())
                .min_decimals(self.min_decimals)
                .max_decimals_opt(self.max_decimals)
                .suffix(self.suffix.clone())
                .prefix(self.prefix.clone());
            if let Some(fmt) = &self.custom_formatter {
                dv = dv.custom_formatter(fmt);
            };
            if let Some(parser) = &self.custom_parser {
                dv = dv.custom_parser(parser);
            }
            dv
        });
        if value != self.get_value() {
            self.set_value(value);
        }
        response
    }

    /// delta(value) / delta(points)
    fn current_gradient(&mut self, position_range: &RangeInclusive<f32>) -> f64 {
        // TODO(emilk): handle clamping
        let value = self.get_value();
        let value_from_pos =
            |position: f32| self.value_from_position(position, position_range.clone());
        let pos_from_value = |value: f64| self.position_from_value(value, position_range.clone());
        let left_value = value_from_pos(pos_from_value(value) - 0.5);
        let right_value = value_from_pos(pos_from_value(value) + 0.5);
        right_value - left_value
    }

    fn add_contents(&mut self, ui: &mut Ui) -> Response {
        let old_value = self.get_value();

        let thickness = ui
            .text_style_height(&TextStyle::Body)
            .at_least(ui.spacing().interact_size.y);
        let mut response = self.allocate_slider_space(ui, thickness);
        self.slider_ui(ui, &response);

        let value = self.get_value();
        response.changed = value != old_value;
        response.widget_info(|| WidgetInfo::slider(value, self.text.text()));

        #[cfg(feature = "accesskit")]
        if let Some(mut node) = ui.ctx().accesskit_node(response.id) {
            use accesskit::Action;
            node.min_numeric_value = Some(*self.range.start());
            node.max_numeric_value = Some(*self.range.end());
            node.numeric_value_step = self.step;
            node.actions |= Action::SetValue;
            let clamp_range = self.clamp_range();
            if value < *clamp_range.end() {
                node.actions |= Action::Increment;
            }
            if value > *clamp_range.start() {
                node.actions |= Action::Decrement;
            }
        }

        let slider_response = response.clone();

        let value_response = if self.show_value {
            let position_range = self.position_range(&response.rect);
            let value_response = self.value_ui(ui, position_range);
            if value_response.gained_focus()
                || value_response.has_focus()
                || value_response.lost_focus()
            {
                // Use the [`DragValue`] id as the id of the whole widget,
                // so that the focus events work as expected.
                response = value_response.union(response);
            } else {
                // Use the slider id as the id for the whole widget
                response = response.union(value_response.clone());
            }
            Some(value_response)
        } else {
            None
        };

        if !self.text.is_empty() {
            let label_response = ui.add(Label::new(self.text.clone()).wrap(false));
            // The slider already has an accessibility label via widget info,
            // but sometimes it's useful for a screen reader to know
            // that a piece of text is a label for another widget,
            // e.g. so the text itself can be excluded from navigation.
            slider_response.labelled_by(label_response.id);
            if let Some(value_response) = value_response {
                value_response.labelled_by(label_response.id);
            }
        }

        response
    }
}

impl<'a> Widget for NotchedSlider<'a> {
    fn ui(mut self, ui: &mut Ui) -> Response {
        let inner_response = match self.orientation {
            SliderOrientation::Horizontal => ui.horizontal(|ui| self.add_contents(ui)),
            SliderOrientation::Vertical => ui.vertical(|ui| self.add_contents(ui)),
        };

        inner_response.inner | inner_response.response
    }
}

// ----------------------------------------------------------------------------
// Helpers for converting slider range to/from normalized [0-1] range.
// Always clamps.
// Logarithmic sliders are allowed to include zero and infinity,
// even though mathematically it doesn't make sense.

/// When the user asks for an infinitely large range (e.g. logarithmic from zero),
/// give a scale that this many orders of magnitude in size.
const INF_RANGE_MAGNITUDE: f64 = 10.0;

fn value_from_normalized(normalized: f64, range: RangeInclusive<f64>, spec: &SliderSpec) -> f64 {
    let (min, max) = (*range.start(), *range.end());

    if min.is_nan() || max.is_nan() {
        f64::NAN
    } else if min == max {
        min
    } else if min > max {
        value_from_normalized(1.0 - normalized, max..=min, spec)
    } else if normalized <= 0.0 {
        min
    } else if normalized >= 1.0 {
        max
    } else if spec.logarithmic {
        if max <= 0.0 {
            // non-positive range
            -value_from_normalized(normalized, -min..=-max, spec)
        } else if 0.0 <= min {
            let (min_log, max_log) = range_log10(min, max, spec);
            let log = lerp(min_log..=max_log, normalized);
            10.0_f64.powf(log)
        } else {
            assert!(min < 0.0 && 0.0 < max);
            let zero_cutoff = logaritmic_zero_cutoff(min, max);
            if normalized < zero_cutoff {
                // negative
                value_from_normalized(
                    remap(normalized, 0.0..=zero_cutoff, 0.0..=1.0),
                    min..=0.0,
                    spec,
                )
            } else {
                // positive
                value_from_normalized(
                    remap(normalized, zero_cutoff..=1.0, 0.0..=1.0),
                    0.0..=max,
                    spec,
                )
            }
        }
    } else {
        assert!(
            min.is_finite() && max.is_finite(),
            "You should use a logarithmic range"
        );
        lerp(range, normalized.clamp(0.0, 1.0))
    }
}

fn normalized_from_value(value: f64, range: RangeInclusive<f64>, spec: &SliderSpec) -> f64 {
    let (min, max) = (*range.start(), *range.end());

    if min.is_nan() || max.is_nan() {
        f64::NAN
    } else if min == max {
        0.5 // empty range, show center of slider
    } else if min > max {
        1.0 - normalized_from_value(value, max..=min, spec)
    } else if value <= min {
        0.0
    } else if value >= max {
        1.0
    } else if spec.logarithmic {
        if max <= 0.0 {
            // non-positive range
            normalized_from_value(-value, -min..=-max, spec)
        } else if 0.0 <= min {
            let (min_log, max_log) = range_log10(min, max, spec);
            let value_log = value.log10();
            remap_clamp(value_log, min_log..=max_log, 0.0..=1.0)
        } else {
            assert!(min < 0.0 && 0.0 < max);
            let zero_cutoff = logaritmic_zero_cutoff(min, max);
            if value < 0.0 {
                // negative
                remap(
                    normalized_from_value(value, min..=0.0, spec),
                    0.0..=1.0,
                    0.0..=zero_cutoff,
                )
            } else {
                // positive side
                remap(
                    normalized_from_value(value, 0.0..=max, spec),
                    0.0..=1.0,
                    zero_cutoff..=1.0,
                )
            }
        }
    } else {
        assert!(
            min.is_finite() && max.is_finite(),
            "You should use a logarithmic range"
        );
        remap_clamp(value, range, 0.0..=1.0)
    }
}

fn range_log10(min: f64, max: f64, spec: &SliderSpec) -> (f64, f64) {
    assert!(spec.logarithmic);
    assert!(min <= max);

    if min == 0.0 && max == f64::INFINITY {
        (spec.smallest_positive.log10(), INF_RANGE_MAGNITUDE)
    } else if min == 0.0 {
        if spec.smallest_positive < max {
            (spec.smallest_positive.log10(), max.log10())
        } else {
            (max.log10() - INF_RANGE_MAGNITUDE, max.log10())
        }
    } else if max == f64::INFINITY {
        if min < spec.largest_finite {
            (min.log10(), spec.largest_finite.log10())
        } else {
            (min.log10(), min.log10() + INF_RANGE_MAGNITUDE)
        }
    } else {
        (min.log10(), max.log10())
    }
}

/// where to put the zero cutoff for logarithmic sliders
/// that crosses zero ?
fn logaritmic_zero_cutoff(min: f64, max: f64) -> f64 {
    assert!(min < 0.0 && 0.0 < max);

    let min_magnitude = if min == -f64::INFINITY {
        INF_RANGE_MAGNITUDE
    } else {
        min.abs().log10().abs()
    };
    let max_magnitude = if max == f64::INFINITY {
        INF_RANGE_MAGNITUDE
    } else {
        max.log10().abs()
    };

    let cutoff = min_magnitude / (min_magnitude + max_magnitude);
    assert!((0.0..=1.0).contains(&cutoff));
    cutoff
}
