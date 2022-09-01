extern crate core;

use adder_codec_rs::transcoder::source::framed_source::FramedSourceBuilder;
use adder_codec_rs::transcoder::source::video::Source;
use adder_codec_rs::SourceCamera;
use rayon::current_num_threads;
use std::error::Error;
use std::io;
use std::io::Write;
use std::time::Instant;
fn main() -> Result<(), Box<dyn Error>> {
    let mut source = FramedSourceBuilder::new(
        "/media/andrew/ExternalM2/LAS/GH010017.mp4".to_string(),
        SourceCamera::FramedU8,
    )
    .frame_start(1420)
    .scale(0.5)
    .communicate_events(true)
    .output_events_filename("/home/andrew/Downloads/events.adder".to_string())
    .color(false)
    .contrast_thresholds(10, 10)
    .show_display(true)
    .time_parameters(5000, 300000, 3000000)
    .finish();

    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(current_num_threads())
        .build()
        .unwrap();
    let mut now = Instant::now();

    let frame_max = 500;

    loop {
        match pool.install(|| source.consume(1)) {
            Ok(_) => {} // Returns Vec<Vec<Event>>, but we're just writing the events out in this example
            Err(e) => {
                println!("Err: {:?}", e);
                break;
            }
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
