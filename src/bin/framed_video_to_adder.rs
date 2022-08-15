extern crate core;

use adder_codec_rs::transcoder::source::framed_source::FramedSource;
use adder_codec_rs::transcoder::source::video::Source;
use adder_codec_rs::SourceCamera;
use rayon::current_num_threads;
use std::error::Error;
use std::io;
use std::io::Write;
use std::time::Instant;
fn main() -> Result<(), Box<dyn Error>> {
    let mut source = FramedSource::new(
        "/home/andrew/Downloads/excerpt.mp4".to_string(),
        1420,
        5000,
        300000,
        3000000,
        0.1,
        0,
        false,
        true,
        20,
        15,
        true,
        true,
        true,
        SourceCamera::FramedU8,
    )
    .unwrap();

    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(current_num_threads())
        .build()
        .unwrap();
    let mut now = Instant::now();

    let frame_max = 500;

    loop {
        match pool.install(|| source.consume(1)) {
            Ok(_) => {} // Returns Vec<Vec<Event>>, but we're just writing the events out in this example
            Err("End of video") => break, // TODO: make it a proper rust error
            Err(_) => {}
        };

        let video = source.get_video();

        if video.in_interval_count % 30 == 0 {
            print!(
                "\rFrame {} in  {}ms",
                video.in_interval_count,
                now.elapsed().as_millis()
            );
            io::stdout().flush().unwrap();
            now = Instant::now();
        }
        if frame_max != 0 && video.in_interval_count >= frame_max {
            break;
        }
    }

    println!("Closing stream...");
    source.get_video_mut().end_write_stream();
    println!("FINISHED");

    Ok(())
}
