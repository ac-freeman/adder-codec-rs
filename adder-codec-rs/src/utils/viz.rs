
use adder_codec_core::{Event, PixelAddress};
#[cfg(feature = "open-cv")]
use {opencv::core::{Mat, MatTraitConst, MatTraitConstManual}, std::io::BufWriter, std::io::Write, std::error::Error};
use std::fs::File;
use std::io;

use std::io::Cursor;
use std::path::Path;
use std::process::{Command, Output};
use video_rs_adder_dep::Frame;

#[cfg(feature = "open-cv")]
/// Writes a given [`Mat`] to a file
/// # Errors
/// * [`io::Error`] if there is an error writing to the file
/// * [`opencv::Error`] if the [`Mat`] is malformed
/// # Safety
/// This function is unsafe because it calls `Mat::at_unchecked()` which is unsafe
/// # Panics
/// This function panics if the amount data written to the file is not equal to the amount of data
/// in the [`Mat`].
pub fn write_frame_to_video_cv(
    frame: &Mat,
    video_writer: &mut BufWriter<File>,
) -> Result<(), Box<dyn Error>> {
    let frame_size = frame.size()?;
    let len = frame_size.width * frame_size.height * frame.channels();

    // SAFETY:
    // `frame` is a valid `Mat` and `len` is the number of elements in the `Mat`
    unsafe {
        for idx in 0..len {
            let val: *const u8 = frame.at_unchecked(idx)? as *const u8;
            let bytes_written = video_writer.write(std::slice::from_raw_parts(val, 1))?;
            assert_eq!(bytes_written, 1);
        }
    }
    Ok(())
}

/// Convenience function for converting binary grayscale data to an mp4. Used for testing.
/// # Errors
/// * [`io::Error`] if there is an error writing to the file
pub fn encode_video_ffmpeg(raw_path: &str, video_path: &str) -> io::Result<Output> {
    // ffmpeg -f rawvideo -pix_fmt gray -s:v 346x260 -r 60 -i ./tmp.gray8 -crf 0 -c:v libx264 ./output_file.mp4
    println!("Writing reconstruction as .mp4 with ffmpeg");
    Command::new("ffmpeg")
        .args([
            "-f", "rawvideo", "-pix_fmt", "gray", "-s:v", "346x260", "-r", "30", "-i", raw_path,
            "-crf", "0", "-c:v", "libx264", "-y", video_path,
        ])
        .output()
}

#[allow(dead_code)]
/// Convenience function for converting downloading a file at the given `video_url`. Used for testing.
/// # Errors
/// * [`io::Error`] if there is an error downloading or writing the file
pub async fn download_file(
    store_path: &str,
    video_url: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Download the video example, if you don't already have it
    let path_str = store_path;
    if !Path::new(path_str).exists() {
        let resp = reqwest::get(video_url).await?;
        let mut file_out = File::create(path_str)?;
        let mut data_in = Cursor::new(resp.bytes().await?);
        std::io::copy(&mut data_in, &mut file_out)?;
    }
    Ok(())
}

/// The display mode for visualizing detected features
#[derive(Debug, PartialEq, Clone, Copy)]
pub enum ShowFeatureMode {
    /// Don't show features at all
    Off,

    /// Show the feature only at the instant in which the pixel **becomes** a feature
    Instant,

    /// Show the feature until it's no longer a feature
    Hold,
}

/// Assuming the given event is a feature, draw it on the given `img` as a white cross
pub fn draw_feature_event(e: &Event, img: &mut Frame) {
    draw_feature_coord(e.coord.x, e.coord.y, img, false, None)
}

/// Draw a cross on the given `img` at the given `x` and `y` coordinates
pub fn draw_feature_coord(
    x: PixelAddress,
    y: PixelAddress,
    img: &mut Frame,
    three_color: bool,
    color: Option<[u8; 3]>,
) {
    let draw_color: [u8; 3] = color.unwrap_or([255, 255, 255]);

    let radius = 2;

    unsafe {
        if three_color {
            for i in -radius..=radius {
                for (c, color) in draw_color.iter().enumerate() {
                    *img.uget_mut(((y as i32 + i) as usize, (x as i32) as usize, c)) = *color;
                    *img.uget_mut(((y as i32) as usize, (x as i32 + i) as usize, c)) = *color;
                }
            }
        } else {
            for i in -radius..=radius {
                *img.uget_mut(((y as i32 + i) as usize, (x as i32) as usize, 0)) = draw_color[0];
                *img.uget_mut(((y as i32) as usize, (x as i32 + i) as usize, 0)) = draw_color[0];
            }
        }
    }
}

/// Draw a rectangle on the given `img` with the given `color`
pub fn draw_rect(
    x1: PixelAddress,
    y1: PixelAddress,
    x2: PixelAddress,
    y2: PixelAddress,
    img: &mut Frame,
    three_color: bool,
    color: Option<[u8; 3]>,
) {
    let draw_color: [u8; 3] = color.unwrap_or([255, 255, 255]);

    unsafe {
        if three_color {
            for i in x1..=x2 {
                for (c, color) in draw_color.iter().enumerate() {
                    *img.uget_mut((y1 as usize, i as usize, c)) = *color;
                    *img.uget_mut((y2 as usize, i as usize, c)) = *color;
                }
            }
            for i in y1..=y2 {
                for (c, color) in draw_color.iter().enumerate() {
                    *img.uget_mut((i as usize, x1 as usize, c)) = *color;
                    *img.uget_mut((i as usize, x2 as usize, c)) = *color;
                }
            }
        } else {
            for i in x1..=x2 {
                *img.uget_mut((y1 as usize, i as usize, 0)) = draw_color[0];
                *img.uget_mut((y2 as usize, i as usize, 0)) = draw_color[0];
            }
            for i in y1..=y2 {
                *img.uget_mut((i as usize, x1 as usize, 0)) = draw_color[0];
                *img.uget_mut((i as usize, x2 as usize, 0)) = draw_color[0];
            }
        }
    }
}
