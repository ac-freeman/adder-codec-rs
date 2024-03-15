extern crate core;

use adder_codec_rs::transcoder::source::video::{Source, VideoBuilder};
use adder_codec_rs::utils::simulproc::{SimulProcArgs, SimulProcessor};

use clap::Parser;
use rayon::current_num_threads;

use std::error::Error;
use std::fs::File;

use adder_codec_core::codec::{EncoderOptions, EncoderType};
use adder_codec_core::SourceCamera::FramedU8;
use adder_codec_core::{PixelMultiMode, TimeMode};
use adder_codec_rs::transcoder::source::framed::Framed;
use std::io::{BufWriter, Cursor};
use std::path::Path;
use std::process::Command;

#[allow(dead_code)]
async fn download_file() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Download the drop.mp4 video example, if you don't already have it
    let path_str = "./tests/samples/videos/drop.mp4";
    if !Path::new(path_str).exists() {
        let resp = reqwest::get("https://www.pexels.com/video/2603664/download/").await?;
        let mut file_out = File::create(path_str)?;
        let mut data_in = Cursor::new(resp.bytes().await?);
        std::io::copy(&mut data_in, &mut file_out)?;
    }
    Ok(())
}

// Scale down source video for comparison
// ffmpeg -i drop.mp4 -vf scale=960:-1 -crf 0 -c:v libx264 drop_scaled.mp4

// Trim scaled video for comparison (500 frames). NOTE starting at frame 1, instead of 0.
// I think this is because OpenCV misses the first frame when decoding.
// Start time corresponds to frame index 1. End time corresponds to frame index 500
// (i.e., 500 frames / 24 FPS)
// ffmpeg -i "./drop_scaled_hd.mp4" -ss 00:00:00.041666667 -t 00:00:20.833333 -crf 0 -c:v libx264 "./drop_scaled_hd_trimmed.mp4

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let mut args: SimulProcArgs = SimulProcArgs::parse();
    if !args.args_filename.is_empty() {
        let content = std::fs::read_to_string(args.args_filename)?;
        args = toml::from_str(&content)?;
    }

    let time_mode = match args.time_mode.to_lowercase().as_str() {
        "delta_t" => TimeMode::DeltaT,
        "absolute" => TimeMode::AbsoluteT,
        "mixed" => TimeMode::Mixed,
        _ => panic!("Invalid time mode"),
    };

    let integration_mode = match args.integration_mode.to_lowercase().as_str() {
        "collapse" => PixelMultiMode::Collapse,
        _ => PixelMultiMode::Normal,
    };
    println!("crf: {}", args.crf);

    //////////////////////////////////////////////////////
    // Overriding the default args for this particular video example.
    // Can comment out if supplying a local file.
    // download_file().await.unwrap();
    // args.input_filename = "./tests/samples/videos/drop.mp4".to_string();
    // args.output_raw_video_filename = "./tests/samples/videos/drop_out".to_string();
    //////////////////////////////////////////////////////

    let mut source: Framed<BufWriter<File>> =
        Framed::new(args.input_filename, args.color_input, args.scale)?
            // .chunk_rows(64)
            .frame_start(args.frame_idx_start)?
            .crf(args.crf)
            .auto_time_parameters(args.ref_time, args.delta_t_max, None)?;

    if !args.output_events_filename.is_empty() {
        let path = Path::new(&args.output_events_filename);
        let file = File::create(path)?;
        let plane = source.get_video_ref().state.plane;
        source = *source.write_out(
            FramedU8,
            time_mode,
            integration_mode,
            Some((args.delta_t_max / args.ref_time) as usize),
            EncoderType::Compressed,
            EncoderOptions::default(plane),
            BufWriter::new(file),
        )?;
    }

    let source_fps = source.source_fps;
    let plane = source.get_video_ref().state.plane;

    let ref_time = source.get_ref_time();
    let num_threads = match args.thread_count {
        0 => current_num_threads(),
        num => num as usize,
    };

    dbg!(args.frame_count_max);
    let mut simul_processor = SimulProcessor::new::<u8>(
        source,
        ref_time,
        args.output_raw_video_filename.as_str(),
        args.frame_count_max as i32,
        num_threads,
        2,
        time_mode,
    )?;

    let now = std::time::Instant::now();
    simul_processor.run(args.frame_count_max)?;
    println!("\n\n{} ms elapsed\n\n", now.elapsed().as_millis());
    let fps = 1000.0
        / (now.elapsed().as_millis() as f64
            / args.frame_count_max.saturating_sub(args.frame_idx_start) as f64);
    println!("{:.1} average frames transcoded per second", fps);

    // Use ffmpeg to encode the raw frame data as an mp4
    let color_str = match args.color_input {
        true => "bgr24",
        _ => "gray",
    };

    let mut ffmpeg = Command::new("sh")
        .arg("-c")
        .arg(
            "ffmpeg -hide_banner -loglevel error -f rawvideo -pix_fmt ".to_owned()
                + color_str
                + " -s:v "
                + plane.w().to_string().as_str()
                + "x"
                + plane.h().to_string().as_str()
                + " -r "
                + source_fps.to_string().as_str()
                + " -i "
                + &args.output_raw_video_filename
                + " -crf 0 -c:v libx264 -y "
                + &args.output_raw_video_filename
                + ".mp4",
        )
        .spawn()?;
    ffmpeg.wait()?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use adder_codec_core::codec::{EncoderOptions, EncoderType};
    use adder_codec_core::SourceCamera::FramedU8;
    use adder_codec_core::TimeMode;
    use adder_codec_core::{DeltaT, PixelMultiMode};
    use adder_codec_rs::transcoder::source::framed::Framed;
    use adder_codec_rs::transcoder::source::video::{Source, VideoBuilder};
    use adder_codec_rs::utils::simulproc::{SimulProcArgs, SimulProcessor};
    use std::error::Error;
    use std::fs;
    use std::fs::File;
    use std::io::BufWriter;
    use std::path::PathBuf;
    use std::process::Command;
    use std::thread::sleep;
    use std::time::Duration;

    #[test]
    fn dark() -> Result<(), Box<dyn Error>> {
        let d = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let manifest_path_str = d.as_path().to_str().unwrap().to_owned();

        let args: SimulProcArgs = SimulProcArgs {
            args_filename: String::new(),
            color_input: false,
            ref_time: 5000,
            delta_t_max: 120_000,
            frame_count_max: 0,
            frame_idx_start: 1,
            show_display: false,
            input_filename: manifest_path_str.clone() + "/tests/samples/lake_scaled_hd_crop.mp4",
            output_events_filename: manifest_path_str.clone()
                + "/tests/samples/TEST_lake_scaled_hd_crop.adder",
            output_raw_video_filename: manifest_path_str
                + "/tests/samples/TEST_lake_scaled_hd_crop",
            scale: 1.0,
            thread_count: 1, // Multithreading causes some issues in testing
            time_mode: "delta_t".to_string(),
            crf: 0,
            integration_mode: "".to_string(),
        };
        let mut source = Framed::new(args.input_filename, args.color_input, args.scale)?
            // .chunk_rows(64)
            .crf(0)
            .time_parameters(5000 * 30, 5000, 120_000, Some(TimeMode::DeltaT))?
            .frame_start(args.frame_idx_start)?;

        let source_fps = source.source_fps as f64;
        source = source.time_parameters(
            (args.ref_time as f64 * source_fps) as DeltaT,
            args.ref_time,
            args.delta_t_max,
            None,
        )?;
        if !args.output_events_filename.is_empty() {
            let file = File::create(args.output_events_filename)?;
            let plane = source.get_video_ref().state.plane;
            let writer = BufWriter::new(file);
            source = *source.write_out(
                FramedU8,
                TimeMode::DeltaT,
                PixelMultiMode::Normal,
                None,
                EncoderType::Raw,
                EncoderOptions::default(plane),
                writer,
            )?;
        }
        let ref_time = source.get_ref_time();

        let mut simul_processor = SimulProcessor::new::<u8>(
            source,
            ref_time,
            args.output_raw_video_filename.as_str(),
            args.frame_count_max as i32,
            1,
            1,
            TimeMode::default(),
        )?;

        simul_processor.run(0).unwrap();
        sleep(Duration::from_secs(5));

        let output_path = "./tests/samples/TEST_lake_scaled_hd_crop";
        assert_eq!(
            fs::metadata(output_path).unwrap().len()
                % (u64::from(simul_processor.source.get_video_ref().state.plane.w())
                    * u64::from(simul_processor.source.get_video_ref().state.plane.h())),
            0
        );

        let output = if !cfg!(target_os = "windows") {
            Command::new("sh")
                .arg("-c")
                .arg("cmp ./tests/samples/TEST_lake_scaled_hd_crop ./tests/samples/lake_scaled_out")
                .output()
                .expect("failed to execute process")
        } else {
            fs::remove_file(output_path).unwrap();
            return Ok(());
        };
        println!("{}", String::from_utf8(output.stdout.clone()).unwrap());

        // Note the file might be larger than that given in ./tests/samples, if the method for
        // framing generates more frames at the end than the original method used. This assertion
        // should still pass if all the frames before that are identical.
        assert_eq!(output.stdout.len(), 0);
        // assert!(output.stdout.len() < 100); // TODO: Make this more precise; figure out if the test has gotten better or worse
        fs::remove_file(output_path).unwrap();

        let output_path = "./tests/samples/TEST_lake_scaled_hd_crop.adder";
        fs::remove_file(output_path)?;
        Ok(())
    }
}
