pub fn write_frame_to_video(frame: &Mat, video_writer: &mut BufWriter<File>) {
    unsafe {
        for idx in 0..frame.size().unwrap().width * frame.size().unwrap().height {
            let val: *const u8 = frame.at_unchecked(idx).unwrap() as *const u8;
            video_writer
                .write(std::slice::from_raw_parts(val, 1))
                .unwrap();
        }
    }
}

use opencv::core::{Mat, MatTraitConstManual};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::process::Command;

pub fn encode_video_ffmpeg(raw_path: &str, video_path: &str) {
    // ffmpeg -f rawvideo -pix_fmt gray -s:v 346x260 -r 60 -i ./tmp.gray8 -crf 0 -c:v libx264 ./output_file.mp4
    println!("Writing reconstruction as .mp4 with ffmpeg");
    Command::new("ffmpeg")
        .args(&[
            "-f", "rawvideo", "-pix_fmt", "gray", "-s:v", "346x260", "-r", "30", "-i", raw_path,
            "-crf", "0", "-c:v", "libx264", "-y", video_path,
        ])
        .output()
        .expect("failed to execute process");
}
