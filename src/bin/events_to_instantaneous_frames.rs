extern crate core;

use std::fs::File;
use std::io;
use std::io::{BufWriter, Write};
use std::time::Instant;
use adder_codec_rs::{Codec};
use adder_codec_rs::framer::event_framer::FramerMode::INSTANTANEOUS;
use adder_codec_rs::framer::event_framer::FrameSequence;
use adder_codec_rs::framer::event_framer::SourceType::U8;
use adder_codec_rs::raw::raw_stream::RawStream;
use adder_codec_rs::framer::event_framer::Framer;

fn main() {
    let input_path = "/home/andrew/Downloads/temppp";
    let mut stream: RawStream = Codec::new();
    stream.open_reader(input_path).expect("Invalid path");
    stream.decode_header();

    let output_path = "/home/andrew/Downloads/temppp_out";
    let mut output_stream = BufWriter::new(File::create(output_path).unwrap());

    let reconstructed_frame_rate = 60;
    // For instantaneous reconstruction, make sure the frame rate matches the source video rate
    assert_eq!(stream.tps / stream.ref_interval, reconstructed_frame_rate);

    let mut frame_sequence: FrameSequence<u8> = FrameSequence::<u8>::new(stream.height.into(), stream.width.into(), stream.channels.into(), stream.tps, reconstructed_frame_rate, INSTANTANEOUS, U8);
    let mut now = Instant::now();
    let mut frame_count = 0;
    loop {
        match stream.decode_event() {
            Ok(event) => {
                if frame_sequence.ingest_event(&event) {
                    match frame_sequence.write_multi_frame_bytes(&mut output_stream) {
                        0 => {panic!("Should have frame, but didn't")}
                        frames_returned => {
                            frame_count += frames_returned;
                            if frame_count % 30 == 0 {
                                print!(
                                    "\rOutput frame {}. Got {} frames in  {}ms",
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



            }
            Err(_e) => {
                eprintln!("\nExiting");
                break
            }
        }
    }

    output_stream.flush().unwrap();
}