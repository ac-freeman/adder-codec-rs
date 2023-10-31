use crate::transcoder::source::video::SourceError;
use adder_codec_core::{Coord, Event, PixelAddress, PlaneSize};
use const_for::const_for;
use ndarray::{s, Array3, ArrayView, Axis, Dimension, Ix2, Ix3, RemoveAxis};
#[cfg(feature = "open-cv")]
use opencv::prelude::KeyPointTraitConst;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::error::Error;
use video_rs::Frame;

// TODO: Explore optimal threshold values
/// The threshold for feature detection
pub const INTENSITY_THRESHOLD: i16 = 30;

/// Indices for the asynchronous FAST 9_16 algorithm
#[rustfmt::skip]
const CIRCLE3: [[isize; 2]; 16] = [
    [0, 3], [1, 3], [2, 2], [3, 1],
    [3, 0], [3, -1], [2, -2], [1, -3],
    [0, -3], [-1, -3], [-2, -2], [-3, -1],
    [-3, 0], [-3, 1], [-2, 2], [-1, 3]
];

const STREAK_SIZE: usize = 9;

const fn threshold_table() -> [u8; 512] {
    let mut table = [0; 512];
    const_for!(i in -255..256 => {
        table[(i + 255) as usize] = if i < -INTENSITY_THRESHOLD {
            1
        } else if i > INTENSITY_THRESHOLD {
            2
        } else {
            0
        };
    });

    table
}

const THRESHOLD_TABLE: [u8; 512] = threshold_table();

/// Check if the given event is a feature
///
/// This implementation is a direct port/adaptation of the OpenCV reference implementation at
/// https://github.com/opencv/opencv_attic/blob/master/opencv/modules/features2d/src/fast.cpp
pub fn is_feature(
    coord: Coord,
    plane: PlaneSize,
    img: &Array3<u8>,
) -> Result<bool, Box<dyn Error>> {
    if coord.is_border(plane.w_usize(), plane.h_usize(), 3) {
        return Ok(false);
    }
    unsafe {
        let candidate: i16 = *img.uget((coord.y_usize(), coord.x_usize(), 0)) as i16;

        let offset = -candidate as isize + 255;
        let tab = THRESHOLD_TABLE.as_ptr().offset(offset);
        debug_assert!(
            (-candidate < INTENSITY_THRESHOLD && *tab == 1)
                || (-candidate > INTENSITY_THRESHOLD && *tab == 2)
                || (-candidate >= -INTENSITY_THRESHOLD
                    && -candidate <= INTENSITY_THRESHOLD
                    && *tab == 0)
        );
        // const uchar* tab = &threshold_tab[0] - v + 2
        let c = plane.c_usize() as isize;
        let width = plane.w() as isize * c;
        // Get a raw pointer to the intensities
        let ptr = img.as_ptr();

        let y = coord.y as isize;
        let x = coord.x as isize;
        debug_assert_eq!(candidate, *ptr.offset((y * width + x * c)) as i16);

        let mut d = *tab
            .offset(*ptr.offset((y + CIRCLE3[0][1]) * width + (x + CIRCLE3[0][0]) * c) as isize)
            | *tab.offset(
                *ptr.offset((y + CIRCLE3[8][1]) * width + (x + CIRCLE3[8][0]) * c) as isize,
            );

        // If both check pixels are within the intensity threshold range, it's not a feature
        if d == 0 {
            return Ok(false);
        }

        // Check other pixels that are on opposite sides of the circle
        d &= *tab
            .offset(*ptr.offset((y + CIRCLE3[2][1]) * width + (x + CIRCLE3[2][0]) * c) as isize)
            | *tab.offset(
                *ptr.offset((y + CIRCLE3[10][1]) * width + (x + CIRCLE3[10][0]) * c) as isize,
            );
        d &= *tab
            .offset(*ptr.offset((y + CIRCLE3[4][1]) * width + (x + CIRCLE3[4][0]) * c) as isize)
            | *tab.offset(
                *ptr.offset((y + CIRCLE3[12][1]) * width + (x + CIRCLE3[12][0]) * c) as isize,
            );
        d &= *tab
            .offset(*ptr.offset((y + CIRCLE3[6][1]) * width + (x + CIRCLE3[6][0]) * c) as isize)
            | *tab.offset(
                *ptr.offset((y + CIRCLE3[14][1]) * width + (x + CIRCLE3[14][0]) * c) as isize,
            );

        // Not a feature
        if d == 0 {
            return Ok(false);
        }

        // Check other pixels that are on opposite sides of the circle
        d &= *tab
            .offset(*ptr.offset((y + CIRCLE3[1][1]) * width + (x + CIRCLE3[1][0]) * c) as isize)
            | *tab.offset(
                *ptr.offset((y + CIRCLE3[9][1]) * width + (x + CIRCLE3[9][0]) * c) as isize,
            );
        d &= *tab
            .offset(*ptr.offset((y + CIRCLE3[3][1]) * width + (x + CIRCLE3[3][0]) * c) as isize)
            | *tab.offset(
                *ptr.offset((y + CIRCLE3[11][1]) * width + (x + CIRCLE3[11][0]) * c) as isize,
            );
        d &= *tab
            .offset(*ptr.offset((y + CIRCLE3[5][1]) * width + (x + CIRCLE3[5][0]) * c) as isize)
            | *tab.offset(
                *ptr.offset((y + CIRCLE3[13][1]) * width + (x + CIRCLE3[13][0]) * c) as isize,
            );
        d &= *tab
            .offset(*ptr.offset((y + CIRCLE3[7][1]) * width + (x + CIRCLE3[7][0]) * c) as isize)
            | *tab.offset(
                *ptr.offset((y + CIRCLE3[15][1]) * width + (x + CIRCLE3[15][0]) * c) as isize,
            );

        if d & 1 > 0 {
            // It's a dark streak
            let vt = candidate - INTENSITY_THRESHOLD;
            let mut count = 0;

            for k in 0..16 {
                let x = *ptr.offset((y + CIRCLE3[k][1]) * width + (x + CIRCLE3[k][0]) * c) as i16;
                if x < vt {
                    count += 1;
                    if count == STREAK_SIZE {
                        return Ok(true);
                    }
                } else {
                    count = 0;
                }
            }
            for k in 16..25 {
                let x = *ptr.offset((y + CIRCLE3[k - 16][1]) * width + (x + CIRCLE3[k - 16][0]) * c)
                    as i16;
                if x < vt {
                    count += 1;
                    if count == STREAK_SIZE {
                        return Ok(true);
                    }
                } else {
                    count = 0;

                    // Then we don't need to check the rest of the circle; can't be a streak long enough
                    if k == 17 {
                        return Ok(false);
                    }
                }
            }
        }

        if d & 2 > 0 {
            // It's a bright streak
            let vt = candidate + INTENSITY_THRESHOLD;
            let mut count = 0;
            for k in 0..16 {
                let x = *ptr.offset((y + CIRCLE3[k][1]) * width + (x + CIRCLE3[k][0]) * c) as i16;
                if x > vt {
                    count += 1;
                    if count == STREAK_SIZE {
                        return Ok(true);
                    }
                } else {
                    count = 0;
                }
            }
            for k in 16..25 {
                let x = *ptr.offset((y + CIRCLE3[k - 16][1]) * width + (x + CIRCLE3[k - 16][0]) * c)
                    as i16;
                if x > vt {
                    count += 1;
                    if count == STREAK_SIZE {
                        return Ok(true);
                    }
                } else {
                    count = 0;

                    // Then we don't need to check the rest of the circle; can't be a streak long enough
                    if k == 17 {
                        return Ok(false);
                    }
                }
            }
        }
    }

    Ok(false)
}

pub fn handle_color(mut input: Frame, color: bool) -> Result<Frame, SourceError> {
    if !color {
        // Map the three color channels to a single grayscale channel
        input
            .exact_chunks_mut((1, 1, 3))
            .into_iter()
            .for_each(|mut v| unsafe {
                *v.uget_mut((0, 0, 0)) = (*v.uget((0, 0, 0)) as f64 * 0.114
                    + *v.uget((0, 0, 1)) as f64 * 0.587
                    + *v.uget((0, 0, 2)) as f64 * 0.299)
                    as u8;
            });

        // Remove the color channels
        input.collapse_axis(Axis(2), 0);
    }
    Ok(input)
}

#[cfg(feature = "open-cv")]
pub fn feature_precision_recall_accuracy(
    gt: &opencv::core::Vector<opencv::core::KeyPoint>,
    prediction: &HashSet<Coord>,
    plane: PlaneSize,
) -> (f64, f64, f64) {
    let (mut tp, mut fp, mut tn, mut fnn) = (0, 0, 0, 0);

    // Channel of first pred event:
    let channel = match prediction.iter().next() {
        None => None,
        Some(coord) => coord.c,
    };

    // convert the keypoints vec to a hashset for convenience
    let mut gt_hash = HashSet::<Coord>::new();
    for keypoint in gt {
        gt_hash.insert(Coord::new(
            keypoint.pt().x as PixelAddress,
            keypoint.pt().y as PixelAddress,
            channel,
        ));
    }

    for y in 0..plane.h() {
        for x in 0..plane.w() {
            let coord = Coord::new(x, y, None);
            if prediction.contains(&coord) {
                if gt_hash.contains(&coord) {
                    tp += 1;
                } else {
                    fp += 1;
                }
            } else {
                if gt_hash.contains(&coord) {
                    fnn += 1;
                } else {
                    tn += 1;
                }
            }
        }
    }

    let precision = (tp as f64) / ((tp + fp) as f64);
    let recall = (tp as f64) / ((tp + fnn) as f64);
    let accuracy = ((tp + tn) as f64) / ((tp + tn + fp + fnn) as f64);
    (precision, recall, accuracy)
}

/// Container for quality metric results
#[derive(Debug, Serialize, Deserialize)]
pub struct QualityMetrics {
    /// Peak signal-to-noise ratio
    pub psnr: Option<f64>,

    /// Mean squared error
    pub mse: Option<f64>,

    /// Structural similarity index measure
    pub ssim: Option<f64>,
}

impl Default for QualityMetrics {
    fn default() -> Self {
        Self {
            psnr: Some(0.0),
            mse: Some(0.0),
            ssim: None,
        }
    }
}

/// Pass in the options for which metrics you want to evaluate by making them Some() in the `results`
/// that you pass in
pub fn calculate_quality_metrics(
    original: &Array3<u8>,
    reconstructed: &Array3<u8>,
    mut results: QualityMetrics,
) -> Result<QualityMetrics, Box<dyn Error>> {
    if original.shape() != reconstructed.shape() {
        return Err("Shapes of original and reconstructed images must match".into());
    }

    let mse = calculate_mse(original, reconstructed)?;
    if results.mse.is_some() {
        results.mse = Some(mse);
    }
    if results.psnr.is_some() {
        results.psnr = Some(calculate_psnr(mse)?);
    }
    if results.ssim.is_some() {
        results.ssim = Some(calculate_ssim(original, reconstructed)?);
    }
    Ok(results)
}

fn calculate_mse(original: &Array3<u8>, reconstructed: &Array3<u8>) -> Result<f64, Box<dyn Error>> {
    if original.shape() != reconstructed.shape() {
        return Err("Shapes of original and reconstructed images must match".into());
    }

    let mut error_sum = 0.0;
    original
        .iter()
        .zip(reconstructed.iter())
        .for_each(|(a, b)| {
            error_sum += (*a as f64 - *b as f64).powi(2);
        });
    Ok(error_sum / (original.len() as f64))
}

fn calculate_psnr(mse: f64) -> Result<f64, Box<dyn Error>> {
    Ok(20.0 * (255.0_f64).log10() - 10.0 * mse.log10())
}

// Below is adapted from https://github.com/ChrisRega/image-compare/blob/main/src/ssim.rs
const DEFAULT_WINDOW_SIZE: usize = 8;
const K1: f64 = 0.01;
const K2: f64 = 0.03;
const L: u8 = u8::MAX;
const C1: f64 = (K1 * L as f64) * (K1 * L as f64);
const C2: f64 = (K2 * L as f64) * (K2 * L as f64);

/// Calculate the SSIM score
fn calculate_ssim(
    original: &Array3<u8>,
    reconstructed: &Array3<u8>,
) -> Result<f64, Box<dyn Error>> {
    let mut scores = vec![];
    for channel in 0..original.shape()[2] {
        let channel_view_original = original.index_axis(Axis(2), channel);
        let channel_view_reconstructed = reconstructed.index_axis(Axis(2), channel);
        let windows_original =
            channel_view_original.windows((DEFAULT_WINDOW_SIZE, DEFAULT_WINDOW_SIZE));
        let windows_reconstructed =
            channel_view_reconstructed.windows((DEFAULT_WINDOW_SIZE, DEFAULT_WINDOW_SIZE));
        let results = windows_original
            .into_iter()
            .zip(windows_reconstructed.into_iter())
            .map(|(w1, w2)| ssim_for_window(w1, w2))
            .collect::<Vec<_>>();
        let score = results
            .iter()
            .map(|r| r * (DEFAULT_WINDOW_SIZE * DEFAULT_WINDOW_SIZE) as f64)
            .sum::<f64>()
            / results
                .iter()
                .map(|r| (DEFAULT_WINDOW_SIZE * DEFAULT_WINDOW_SIZE) as f64)
                .sum::<f64>();
        scores.push(score)
    }

    let score = (scores.iter().sum::<f64>() / scores.len() as f64) * 100.0;

    debug_assert!(score >= 0.0);
    debug_assert!(score <= 100.0);

    Ok(score)
}

fn ssim_for_window(source_window: ArrayView<u8, Ix2>, recon_window: ArrayView<u8, Ix2>) -> f64 {
    let mean_x = mean(&source_window);
    let mean_y = mean(&recon_window);
    let variance_x = covariance(&source_window, mean_x, &source_window, mean_x);
    let variance_y = covariance(&recon_window, mean_y, &recon_window, mean_y);
    let covariance = covariance(&source_window, mean_x, &recon_window, mean_y);
    let counter = (2. * mean_x * mean_y + C1) * (2. * covariance + C2);
    let denominator = (mean_x.powi(2) + mean_y.powi(2) + C1) * (variance_x + variance_y + C2);
    counter / denominator
}

fn covariance(
    window_x: &ArrayView<u8, Ix2>,
    mean_x: f64,
    window_y: &ArrayView<u8, Ix2>,
    mean_y: f64,
) -> f64 {
    window_x
        .iter()
        .zip(window_y.iter())
        .map(|(x, y)| (*x as f64 - mean_x) * (*y as f64 - mean_y))
        .sum::<f64>()
}

fn mean(window: &ArrayView<u8, Ix2>) -> f64 {
    let sum = window.iter().map(|pixel| *pixel as f64).sum::<f64>();

    sum / window.len() as f64
}
