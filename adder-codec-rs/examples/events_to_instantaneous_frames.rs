extern crate core;

use adder_codec_core::codec::decoder::Decoder;
use adder_codec_core::codec::raw::stream::RawInput;
use adder_codec_core::codec::ReadCompression;
use adder_codec_rs::framer::driver::Framer;
use adder_codec_rs::framer::driver::FramerMode::INSTANTANEOUS;
use adder_codec_rs::framer::driver::{FrameSequence, FramerBuilder};
use bitstream_io::{BigEndian, BitReader};
use std::fs::File;
use std::io;
use std::io::{BufReader, BufWriter, Write};
use std::time::Instant;

fn main() {
    let input_path = "/home/andrew/Downloads/tmp_events_migrated.adder";
    let tmp = File::open(input_path).unwrap();
    let bufreader = BufReader::new(tmp);
    let compression = RawInput::new();

    let mut bitreader = BitReader::endian(bufreader, BigEndian);
    let mut reader = Decoder::new_raw(compression, &mut bitreader).unwrap();

    let output_path = "/home/andrew/Downloads/temppp_out";
    let mut output_stream = BufWriter::new(File::create(output_path).unwrap());

    let reconstructed_frame_rate = f64::from(reader.meta().tps / reader.meta().ref_interval);
    println!("reconstructed_frame_rate: {reconstructed_frame_rate}");
    // For instantaneous reconstruction, make sure the frame rate matches the source video rate
    // assert_eq!(
    //     stream.tps / stream.ref_interval,
    //     reconstructed_frame_rate as u32
    // );

    let mut frame_sequence: FrameSequence<u8> =
        FramerBuilder::new(reader.meta().plane.clone(), 260)
            .codec_version(reader.meta().codec_version, reader.meta().time_mode)
            .time_parameters(
                reader.meta().tps,
                reader.meta().ref_interval,
                reader.meta().delta_t_max,
                reconstructed_frame_rate,
            )
            .mode(INSTANTANEOUS)
            .source(reader.get_source_type(), reader.meta().source_camera)
            .finish();

    let mut now = Instant::now();
    let mut frame_count = 0;
    loop {
        match reader.digest_event(&mut bitreader) {
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
