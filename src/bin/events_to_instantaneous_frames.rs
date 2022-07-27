extern crate core;

use std::fs::File;
use std::io;
use std::io::{BufWriter, Error, Write};
use std::time::Instant;
use adder_codec_rs::{Codec, D_MAX, Event};
use adder_codec_rs::framer::array3d::Array3DError;
use adder_codec_rs::framer::framer::FramerMode::INSTANTANEOUS;
use adder_codec_rs::framer::framer::FrameSequence;
use adder_codec_rs::framer::framer::SourceType::U8;
use adder_codec_rs::raw::raw_stream::RawStream;
use adder_codec_rs::framer::framer::Framer;

fn main() -> Result<(), Array3DError> {
    let input_path = "/home/andrew/Downloads/temppp";
    let mut stream: RawStream = Codec::new();
    stream.open_reader(input_path.to_string());
    stream.decode_header();

    let output_path = "/home/andrew/Downloads/temppp_out";
    let mut output_stream = BufWriter::new(File::create(output_path.to_string()).unwrap());

    let mut frame_sequence: FrameSequence<u8> = FrameSequence::<u8>::new(stream.height.into(), stream.width.into(), stream.channels.into(), stream.tps, 60, D_MAX, stream.delta_t_max, INSTANTANEOUS, U8);
    let mut now = Instant::now();
    let mut frame_count = 0;
    loop {
        match stream.decode_event() {
            Ok(event) => {
                if frame_sequence.ingest_event(&event)? {
                    match frame_sequence.get_frame_bytes() {
                        None => { panic!("should have frame") },
                        Some(bytes) => {
                            match output_stream.write_all(&bytes) {
                                Ok(_) => {},
                                Err(e) => {panic!("{}", e)}
                            }
                            frame_count += 1;
                            if frame_count % 30 == 0 {
                                print!(
                                    "\rOutput frame {} in  {}ms",
                                    frame_count,
                                    now.elapsed().as_millis()
                                );
                                io::stdout().flush().unwrap();
                                now = Instant::now();
                            }
                        }
                    }
                }



            }
            Err(e) => {
                eprintln!("{}", e);
                break
            }
        }
    }

    output_stream.flush();
    Ok(())
}