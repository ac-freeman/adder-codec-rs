use bevy::asset::{Assets, Handle};
use bevy::prelude::Color::Rgba;
use bevy::prelude::{Image, ResMut};
use bevy::render::render_resource::TextureFormat::Rgba8Uint;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy_egui::egui::plot::{Line, PlotPoints};
use futures::StreamExt;
use ndarray::{Array, Axis};
use std::collections::VecDeque;
use std::error::Error;
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

pub fn prep_bevy_image(
    mut image_mat: Frame,
    color: bool,
    width: u16,
    height: u16,
) -> Result<Image, Box<dyn Error>> {
    // let image_bgra = if color {
    //     // Swap the red and blue channels
    //     // image_mat.swap_axes(2, 0);
    //     //
    //     // let temp = image_mat.index_axis_mut(Axis(2), 0).to_owned();
    //     // let blue_channel = image_mat.index_axis_mut(Axis(2), 2).to_owned();
    //     // image_mat.index_axis_mut(Axis(2), 0).assign(&blue_channel);
    //     // // Swap the channels by copying
    //     // image_mat.index_axis_mut(Axis(2), 2).assign(&temp);
    //
    //
    //
    //     ndarray::par_azip!((red in &mut image_mat.index_axis_mut(Axis(2), 0), blue in &mut image_mat.index_axis_mut(Axis(2), 2)) {
    //         std::mem::swap(red, blue);
    //     });
    //
    //     // add alpha channel
    //     ndarray::concatenate(
    //         Axis(2),
    //         &[
    //             image_mat.clone().view(),
    //             Array::from_elem((image_mat.shape()[0], image_mat.shape()[1], 1), 255).view(),
    //         ],
    //     )?
    // } else {
    //     ndarray::concatenate(
    //         Axis(2),
    //         &[
    //             image_mat.clone().view(),
    //             image_mat.clone().view(),
    //             image_mat.clone().view(),
    //             Array::from_elem((image_mat.shape()[0], image_mat.shape()[1], 1), 255).view(),
    //         ],
    //     )?
    // };

    // let view = Assets::get_mut(last_view)?;
    let image_mat = image_mat.as_standard_layout();

    // Preallocate space for the new vector
    let mut new_image_mat = Vec::with_capacity(width as usize * height as usize * 4);

    let mut image_mat = image_mat.into_owned().into_raw_vec();
    if color {
        // Iterate over chunks of 3 elements and insert the value after each chunk
        for chunk in image_mat.chunks(3) {
            new_image_mat.extend(chunk.iter().cloned());
            new_image_mat.push(255);
        }
    } else {
        for chunk in image_mat.chunks(1) {
            new_image_mat.extend(chunk.iter().cloned());
            new_image_mat.extend(chunk.iter().cloned());
            new_image_mat.extend(chunk.iter().cloned());
            new_image_mat.push(255);
        }
    }

    // image_mat.resize((width as usize * height as usize * 4) as usize, 255); // Add the alpha channel
    // let mut char_array = vec!['\0'; image_mat.len()];
    // let src_ptr = arr.as_ptr();
    // let dest_ptr = char_array.as_mut_ptr();
    //
    // // Calculate the number of elements
    // let num_elements = arr.len();
    //
    // // Use unsafe to copy the elements
    // unsafe {
    //     std::ptr::copy_nonoverlapping(src_ptr, dest_ptr, num_elements);
    // }

    Ok(Image::new(
        Extent3d {
            width: width.into(),
            height: height.into(),
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        new_image_mat,
        TextureFormat::Rgba8UnormSrgb,
    ))
}

pub fn prep_bevy_image2(
    mut image_mat: Frame,
    mut images: ResMut<Assets<Image>>,
    mut last_view: &mut Handle<Image>,
    color: bool,
    width: usize,
    height: usize,
) -> Result<Image, Box<dyn Error>> {
    // TODO: use  reinterpret_stacked_2d_as_array from bevy image
    let tmp = images.get_mut(last_view);
    let num_channels: usize = if color { 3 } else { 1 };

    if let Some(img) = tmp {
        let view = &mut img.data;

        let mut px_count: usize = 0;
        // dbg!(view.len());
        let mut last_color = 0;
        for (idx, px) in view.iter_mut().enumerate() {
            if idx % 3 == 0 {
                *px = 255;
            } else if !color && idx % 4 != 0 {
                *px = last_color;
            } else {
                // println!("px_count: {}", px_count);

                *px = image_mat[[
                    px_count / (width * num_channels),
                    (px_count % (width * num_channels)) / num_channels,
                    px_count % num_channels,
                ]];
                if !color {
                    last_color = *px;
                }
                px_count += 1;
            }
        }
        // *last_view = images.add(*img);
    } else {
        // let view = Assets::get_mut(last_view)?;
        let image_mat = image_mat.as_standard_layout();

        // Preallocate space for the new vector
        let mut new_image_mat = Vec::with_capacity(width as usize * height as usize * 4);

        let mut image_mat = image_mat.into_owned().into_raw_vec();
        if color {
            // Iterate over chunks of 3 elements and insert the value after each chunk
            for chunk in image_mat.chunks(3) {
                new_image_mat.extend(chunk.iter().cloned());
                new_image_mat.push(255);
            }
        } else {
            for chunk in image_mat.chunks(1) {
                new_image_mat.extend(chunk.iter().cloned());
                new_image_mat.extend(chunk.iter().cloned());
                new_image_mat.extend(chunk.iter().cloned());
                new_image_mat.push(255);
            }
        }
        let image = Image::new(
            Extent3d {
                width: width as u32,
                height: height as u32,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            new_image_mat,
            TextureFormat::Rgba8UnormSrgb,
        );
        *last_view = images.add(image);
    }

    // let view = images
    //     .get_mut(last_view)
    //     .unwrap()
    //     .try_into_dynamic()?
    //     .as_mut_rgba8();

    // if let Some(img) = view {
    //     for (x, y, px) in img.enumerate_pixels_mut() {
    //         px.0 = [
    //             image_mat[[y as usize, x as usize, 0]],
    //             image_mat[[y as usize, x as usize, 1]],
    //             image_mat[[y as usize, x as usize, 2]],
    //             255,
    //         ];
    //     }
    // } else {
    //     panic!("todo")
    // }
    // let image_mat = image_mat.as_standard_layout();
    //
    // // Preallocate space for the new vector
    // // let mut new_image_mat = Vec::with_capacity(width as usize * height as usize * 4);
    //
    // let mut image_mat = image_mat.into_owned().into_raw_vec();
    // if color {
    //     for px in view.iter_mut() {
    //         let r = image_mat.pop().unwrap();
    //         let g = image_mat.pop().unwrap();
    //         let b = image_mat.pop().unwrap();
    //         *px = bevy::render::color::Color::rgba(r, g, b, a);
    //     }
    //
    //     // // Iterate over chunks of 3 elements and insert the value after each chunk
    //     // for chunk in image_mat.chunks(3) {
    //     //     new_image_mat.extend(chunk.iter().cloned());
    //     //     new_image_mat.push(255);
    //     // }
    // } else {
    //     // for chunk in image_mat.chunks(1) {
    //     //     new_image_mat.extend(chunk.iter().cloned());
    //     //     new_image_mat.extend(chunk.iter().cloned());
    //     //     new_image_mat.extend(chunk.iter().cloned());
    //     //     new_image_mat.push(255);
    //     // }
    // }

    // image_mat.resize((width as usize * height as usize * 4) as usize, 255); // Add the alpha channel
    // let mut char_array = vec!['\0'; image_mat.len()];
    // let src_ptr = arr.as_ptr();
    // let dest_ptr = char_array.as_mut_ptr();
    //
    // // Calculate the number of elements
    // let num_elements = arr.len();
    //
    // // Use unsafe to copy the elements
    // unsafe {
    //     std::ptr::copy_nonoverlapping(src_ptr, dest_ptr, num_elements);
    // }

    Err("todo".into())
    // Ok(Image::new(
    //     Extent3d {
    //         width: width.into(),
    //         height: height.into(),
    //         depth_or_array_layers: 1,
    //     },
    //     TextureDimension::D2,
    //     new_image_mat,
    //     TextureFormat::Rgba8UnormSrgb,
    // ))
}
