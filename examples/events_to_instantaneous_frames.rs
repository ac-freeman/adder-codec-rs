extern crate core;

use adder_codec_rs::framer::event_framer::Framer;
use adder_codec_rs::framer::event_framer::FramerMode::INSTANTANEOUS;
use adder_codec_rs::framer::event_framer::{FrameSequence, FramerBuilder};
use adder_codec_rs::raw::raw_stream::RawStream;
use adder_codec_rs::Codec;
use std::fs::File;
use std::io;
use std::io::{BufWriter, Write};
use std::time::Instant;

fn main() {
    let input_path = "/mnt/tmp/tmp_events.adder";
    let mut stream: RawStream = Codec::new();
    stream.open_reader(input_path).expect("Invalid path");
    stream.decode_header().expect("Invalid header");

    let output_path = "/mnt/tmp/temppp_out";
    let mut output_stream = BufWriter::new(File::create(output_path).unwrap());

    let reconstructed_frame_rate = 500.0;
    // For instantaneous reconstruction, make sure the frame rate matches the source video rate
    assert_eq!(
        stream.tps / stream.ref_interval,
        reconstructed_frame_rate as u32
    );

    let mut frame_sequence: FrameSequence<u8> = FramerBuilder::new(
        stream.height.into(),
        stream.width.into(),
        stream.channels.into(),
        260,
    )
    .codec_version(stream.codec_version)
    .time_parameters(stream.tps, stream.ref_interval, reconstructed_frame_rate)
    .mode(INSTANTANEOUS)
    .source(stream.get_source_type(), stream.source_camera)
    .finish();

    let mut now = Instant::now();
    let mut frame_count = 0;
    loop {
        match stream.decode_event() {
            Ok(mut event) => {
                if frame_sequence.ingest_event(&mut event) {
                    match frame_sequence.write_multi_frame_bytes(&mut output_stream) {
                        0 => {
                            panic!("Should have frame, but didn't")
                        }
                        frames_returned => {
                            frame_count += frames_returned;
                            print!(
                                "\rOutput frame {}. Got {} frames in  {}ms\t",
                                frame_count,
                                frames_returned,
                                now.elapsed().as_millis()
                            );
                            io::stdout().flush().unwrap();
                            now = Instant::now();
                        }
                    }
                }
            }
            Err(_e) => {
                eprintln!("\nExiting");
                break;
            }
        }
    }

    output_stream.flush().unwrap();
}
