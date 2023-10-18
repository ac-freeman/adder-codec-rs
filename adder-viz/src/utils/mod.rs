use bevy::prelude::Image;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy_egui::egui::plot::{Line, PlotPoints};
use ndarray::{Array, Axis};
use std::collections::VecDeque;
use std::error::Error;
use video_rs::Frame;

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
        if new_point.is_finite() {
            self.points.push_back(new_point);
        } else {
            self.points.push_back(0.0);
        }
        self.points.pop_front();
    }
}

pub fn prep_bevy_image(
    mut image_mat: Frame,
    color: bool,
    width: u16,
    height: u16,
) -> Result<Image, Box<dyn Error>> {
    let image_bgra = if color {
        // Swap the red and blue channels
        let temp = image_mat.index_axis_mut(Axis(2), 0).to_owned();
        let blue_channel = image_mat.index_axis_mut(Axis(2), 2).to_owned();
        image_mat.index_axis_mut(Axis(2), 0).assign(&blue_channel);
        // Swap the channels by copying
        image_mat.index_axis_mut(Axis(2), 2).assign(&temp);

        // add alpha channel
        ndarray::concatenate(
            Axis(2),
            &[
                image_mat.clone().view(),
                Array::from_elem((image_mat.shape()[0], image_mat.shape()[1], 1), 255).view(),
            ],
        )?
    } else {
        ndarray::concatenate(
            Axis(2),
            &[
                image_mat.clone().view(),
                image_mat.clone().view(),
                image_mat.clone().view(),
                Array::from_elem((image_mat.shape()[0], image_mat.shape()[1], 1), 255).view(),
            ],
        )?
    };
    let image_bgra = image_bgra.as_standard_layout();

    Ok(Image::new(
        Extent3d {
            width: width.into(),
            height: height.into(),
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        Vec::from(image_bgra.as_slice().unwrap()),
        TextureFormat::Bgra8UnormSrgb,
    ))
}
