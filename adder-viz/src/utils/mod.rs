use eframe::epaint::ColorImage;
use std::collections::VecDeque;
use std::error::Error;
use video_rs_adder_dep::Frame;

pub(crate) mod slider;

// pub(crate) struct PlotY {
//     pub points: VecDeque<Option<f64>>,
// }
//
// impl PlotY {
//     pub(crate) fn get_plotline(&self, name: &str, log_base: bool) -> Line {
//         let plot_points: PlotPoints = (0..1000)
//             .map(|i| {
//                 let x = i as f64;
//                 let y = self.points[i].unwrap_or(0.0);
//                 if log_base && y > 0.0 {
//                     [x, y.log10()]
//                 } else {
//                     [x, y]
//                 }
//             })
//             .collect();
//         Line::new(plot_points).name(name)
//     }
//
//     pub(crate) fn update(&mut self, new_opt: Option<f64>) {
//         match new_opt {
//             Some(new) => {
//                 if new.is_finite() {
//                     self.points.push_back(Some(new));
//                 } else {
//                     self.points.push_back(Some(0.0));
//                 }
//             }
//             None => self.points.push_back(None),
//         }
//         self.points.pop_front();
//     }
// }

#[inline]
pub fn prep_epaint_image(
    image_mat: &Frame,
    color: bool,
    width: usize,
    height: usize,
) -> Result<ColorImage, Box<dyn Error>> {
    if !color {
        return Ok(ColorImage::from_gray(
            [width, height],
            image_mat.as_standard_layout().as_slice().unwrap(),
        ));
    } else {
        return Ok(ColorImage::from_rgb(
            [width, height],
            image_mat.as_standard_layout().as_slice().unwrap(),
        ));
    }
}
