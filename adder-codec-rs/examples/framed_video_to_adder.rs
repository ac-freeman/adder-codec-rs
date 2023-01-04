extern crate core;

use adder_codec_rs::transcoder::source::framed::Framed;
use adder_codec_rs::transcoder::source::video::{Source, VideoBuilder};

use adder_codec_rs::SourceCamera::FramedU8;
use rayon::current_num_threads;
use std::error::Error;
use std::io;
use std::io::Write;
use std::time::Instant;

fn main() -> Result<(), Box<dyn Error>> {
    let mut source = Framed::new(
        "/media/andrew/ExternalM2/LAS/GH010017.mp4".to_string(),
        false,
        0.5,
    )?
    .frame_start(1420)?
    .write_out("/home/andrew/Downloads/events.adder".to_string(), FramedU8)?
    .contrast_thresholds(10, 10)
    .show_display(true)
    .auto_time_parameters(255, 255 * 30);

    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(current_num_threads())
        .build()
        .unwrap();
    let mut now = Instant::now();

    let frame_max = 500;

    loop {
        match source.consume(1, &pool) {
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
