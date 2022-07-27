use std::io::Error;
use adder_codec_rs::{Codec, D_MAX, Event};
use adder_codec_rs::framer::framer::FramerMode::INSTANTANEOUS;
use adder_codec_rs::framer::framer::FrameSequence;
use adder_codec_rs::framer::framer::SourceType::U8;
use adder_codec_rs::raw::raw_stream::RawStream;
use adder_codec_rs::framer::framer::Framer;

fn main() {
    let input_path = "/home/andrew/Downloads/temppp";
    let mut stream: RawStream = Codec::new();
    stream.open_reader(input_path.to_string());
    stream.decode_header();

    let mut frame_sequence: FrameSequence<u8> = FrameSequence::<u8>::new(stream.height.into(), stream.width.into(), stream.channels.into(), stream.tps, 60, D_MAX, stream.delta_t_max, INSTANTANEOUS, U8);
    loop {
        match stream.decode_event() {
            Ok(event) => {
                if frame_sequence.ingest_event(&event).unwrap() {
                    println!("frame filled")
                }



            }
            Err(e) => {panic!("{}", e)}
        }
    }
}