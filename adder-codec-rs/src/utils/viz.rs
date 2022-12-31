use opencv::core::{Mat, MatTraitConst, MatTraitConstManual};
use std::error::Error;
use std::fs::File;
use std::io;
use std::io::{BufWriter, Cursor, Write};
use std::path::Path;
use std::process::{Command, Output};

pub fn write_frame_to_video(
    frame: &Mat,
    video_writer: &mut BufWriter<File>,
) -> Result<(), Box<dyn Error>> {
    let frame_size = frame.size()?;
    let len = frame_size.width * frame_size.height * frame.channels();

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
