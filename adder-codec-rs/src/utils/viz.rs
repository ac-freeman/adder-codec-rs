use adder_codec_core::{Event, PixelAddress};
use opencv::core::{Mat, MatTrait, MatTraitConst, MatTraitConstManual};
use std::error::Error;
use std::fs::File;
use std::io;
use std::io::{BufWriter, Cursor, Write};
use std::path::Path;
use std::process::{Command, Output};
use video_rs::Frame;

/// Writes a given [`Mat`] to a file
/// # Errors
/// * [`io::Error`] if there is an error writing to the file
/// * [`opencv::Error`] if the [`Mat`] is malformed
/// # Safety
/// This function is unsafe because it calls `Mat::at_unchecked()` which is unsafe
/// # Panics
/// This function panics if the amount data written to the file is not equal to the amount of data
/// in the [`Mat`].
pub fn write_frame_to_video(
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
            assert_eq!(
                video_writer.write(std::slice::from_raw_parts(val, 1))?,
                len as usize
            );
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
#[derive(Debug, PartialEq, Clone, Copy)]
pub enum ShowFeatureMode {
    Off,
    Instant,
    Hold,
}

/// Assuming the given event is a feature, draw it on the given `img` as a white cross
pub fn draw_feature_event(e: &Event, img: &mut Frame) -> Result<(), opencv::Error> {
    draw_feature_coord(e.coord.x, e.coord.y, img, false)
}

pub fn draw_feature_coord(
    x: PixelAddress,
    y: PixelAddress,
    img: &mut Frame,
    color: bool,
) -> Result<(), opencv::Error> {
    let draw_color: u8 = 255;
    let radius = 2;

    unsafe {
        if color {
            for i in -radius..=radius {
                for c in 0..3 {
                    *img.uget_mut(((y as i32 + i) as usize, (x as i32) as usize, c)) = draw_color;
                    *img.uget_mut(((y as i32) as usize, (x as i32 + i) as usize, c)) = draw_color;
                }
            }
        } else {
            for i in -radius..=radius {
                *img.uget_mut(((y as i32 + i) as usize, (x as i32) as usize, 0)) = draw_color;
                *img.uget_mut(((y as i32) as usize, (x as i32 + i) as usize, 0)) = draw_color;
            }
        }
    }

    Ok(())
}
