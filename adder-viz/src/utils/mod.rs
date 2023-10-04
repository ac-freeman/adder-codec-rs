use bevy_egui::egui::plot::{Line, PlotPoints};
use std::collections::VecDeque;

pub(crate) mod slider;

pub(crate) struct PlotY {
    pub points: VecDeque<f64>,
}

impl PlotY {
    pub(crate) fn get_plotline(&self, name: &str) -> Line {
        let plot_points: PlotPoints = (0..1000)
            .map(|i| {
                let x = i as f64;
                [x, self.points[i]]
            })
            .collect();
        Line::new(plot_points).name(name)
    }

    pub(crate) fn update(&mut self, new_point: f64) {
        self.points.push_back(new_point);
        self.points.pop_front();
    }
}
