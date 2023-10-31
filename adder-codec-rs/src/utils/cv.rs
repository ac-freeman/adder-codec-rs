use crate::transcoder::source::video::SourceError;
use adder_codec_core::{Coord, Event, PlaneSize};
use const_for::const_for;
use ndarray::{s, Array3, ArrayView, Axis, Dimension, Ix2, Ix3, RemoveAxis};
use serde::{Deserialize, Serialize};
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
/// count_filter: whether or not to perform the quick check of 4 pixels in the circle (OpenCV implementation by default doesn't do this)
pub fn is_feature(
    coord: Coord,
    plane: PlaneSize,
    img: &Array3<u8>,
    // spot_check: bool,
) -> Result<bool, Box<dyn Error>> {
    if coord.is_border(plane.w_usize(), plane.h_usize(), 3) {
        return Ok(false);
    }
    unsafe {
        let candidate: i16 = *img.uget((coord.y_usize(), coord.x_usize(), 0)) as i16;
        let tab: *const u8 =
            THRESHOLD_TABLE.as_ptr().offset((255 - candidate) as isize) as *const u8;
        // const uchar* tab = &threshold_tab[0] - v + 255;

        let width = plane.w() as isize;
        // Get a raw pointer to the intensities
        let ptr = img.as_ptr();

        let y = coord.y as isize;
        let x = coord.x as isize;

        let mut count = 0;
        if (*ptr.offset((y + CIRCLE3[4][1]) * width + x + CIRCLE3[4][0]) as i16 - candidate).abs()
            > INTENSITY_THRESHOLD
        {
            count += 1;
        }
        if (*ptr.offset((y + CIRCLE3[12][1]) * width + x + CIRCLE3[12][0]) as i16 - candidate).abs()
            > INTENSITY_THRESHOLD
        {
            count += 1;
        }
        if (*ptr.offset((y + CIRCLE3[1][1]) * width + x + CIRCLE3[1][0]) as i16 - candidate).abs()
            > INTENSITY_THRESHOLD
        {
            count += 1;
        }

        if count == 0 {
            return Ok(false);
        }

        if (*ptr.offset((y + CIRCLE3[7][1]) * width + x + CIRCLE3[7][0]) as i16 - candidate).abs()
            > INTENSITY_THRESHOLD
        {
            count += 1;
        }

        if count <= 1 {
            return Ok(false);
        }

        for i in 0..16 {
            // Bright or dark streak?
            let brighter =
                *ptr.offset((y + CIRCLE3[i][1]) * width + CIRCLE3[i][0]) as i16 > candidate;

            let mut did_break = false;

            for j in 0..STREAK_SIZE {
                if brighter {
                    if *ptr.offset(
                        (y + CIRCLE3[(i + j) % 16][1]) * width + x + CIRCLE3[(i + j) % 16][0],
                    ) as i16
                        <= candidate + INTENSITY_THRESHOLD
                    {
                        did_break = true;
                    }
                } else if *ptr
                    .offset((y + CIRCLE3[(i + j) % 16][1]) * width + x + CIRCLE3[(i + j) % 16][0])
                    as i16
                    >= candidate - INTENSITY_THRESHOLD
                {
                    did_break = true;
                }
            }

            if !did_break {
                return Ok(true);
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
