extern crate core;

use adder_codec_rs::framer::driver::Framer;
use adder_codec_rs::framer::driver::FramerMode::INSTANTANEOUS;
use adder_codec_rs::framer::driver::{FrameSequence, FramerBuilder};
use adder_codec_rs::raw::stream::Raw;
use adder_codec_rs::Codec;
use std::fs::File;
use std::io;
use std::io::{BufWriter, Write};
use std::time::Instant;

fn main() {
    let input_path = "/home/andrew/Downloads/hjkhjkl_v2.adder";
    let mut stream: Raw = Codec::new();
    stream.open_reader(input_path).expect("Invalid path");
    stream.decode_header().expect("Invalid header");

    let output_path = "/home/andrew/Downloads/temppp_out";
    let mut output_stream = BufWriter::new(File::create(output_path).unwrap());

    let reconstructed_frame_rate = f64::from(stream.tps / stream.ref_interval);
    println!("reconstructed_frame_rate: {reconstructed_frame_rate}");
    // For instantaneous reconstruction, make sure the frame rate matches the source video rate
    // assert_eq!(
    //     stream.tps / stream.ref_interval,
    //     reconstructed_frame_rate as u32
    // );

    let mut frame_sequence: FrameSequence<u8> = FramerBuilder::new(stream.plane.clone(), 260)
        .codec_version(stream.codec_version, stream.time_mode)
        .time_parameters(
            stream.tps,
            stream.ref_interval,
            stream.delta_t_max,
            reconstructed_frame_rate,
        )
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
                        Ok(0) => {
                            panic!("Should have frame, but didn't")
                        }
                        Ok(frames_returned) => {
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
                        Err(e) => {
                            eprintln!("Error writing frame: {e}");
                            break;
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
