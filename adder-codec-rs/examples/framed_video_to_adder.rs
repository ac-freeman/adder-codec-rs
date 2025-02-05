extern crate core;

use adder_codec_core::codec::{EncoderOptions, EncoderType};
use adder_codec_core::SourceCamera::FramedU8;
use adder_codec_core::{PixelMultiMode, TimeMode};
use adder_codec_rs::transcoder::source::framed::Framed;
use adder_codec_rs::transcoder::source::video::{Source, VideoBuilder};
use rayon::current_num_threads;
use std::error::Error;
use std::fs::File;
use std::io;
use std::io::{BufWriter, Write};
use std::time::Instant;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let file = File::create("/home/andrew/Downloads/events.adder")?;
    let writer = BufWriter::new(file);

    let mut source: Framed<BufWriter<File>> = Framed::new(
        "/media/andrew/ExternalM2/LAS/GH010017.mp4"
            .to_string()
            .parse()
            .unwrap(),
        false,
        0.5,
    )?;
    let plane = source.get_video_ref().state.plane;
    source = source
        .frame_start(1420)?
        .write_out(
            FramedU8,
            TimeMode::DeltaT,
            PixelMultiMode::Normal,
            Some(30),
            EncoderType::Raw,
            EncoderOptions::default(plane),
            writer,
        )?
        .auto_time_parameters(255, 255 * 30, None)?;

    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(current_num_threads())
        .build()
        .unwrap();
    let mut now = Instant::now();

    let frame_max = 500;

    loop {
        match source.consume() {
            Ok(_) => {} // Returns Vec<Vec<Event>>, but we're just writing the events out in this example
            Err(e) => {
                println!("Err: {e:?}");
                break;
            }
        };

        let video = source.get_video_ref();

        if video.state.in_interval_count % 30 == 0 {
            print!(
                "\rFrame {} in  {}ms",
                video.state.in_interval_count,
                now.elapsed().as_millis()
            );
            io::stdout().flush().unwrap();
            now = Instant::now();
        }
        if frame_max != 0 && video.state.in_interval_count >= frame_max {
            break;
        }
    }

    println!("Closing stream...");
    source.get_video_mut().end_write_stream().unwrap();
    println!("FINISHED");

    Ok(())
}
