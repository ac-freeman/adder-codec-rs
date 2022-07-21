extern crate adder_codec_rs;

use std::fs;
use adder_codec_rs::Codec;
use adder_codec_rs::raw::raw_stream::RawStream;

#[test]
#[should_panic]
fn test_encode_header_non_init() {
    let mut stream: RawStream = Codec::new();
    stream.serialize_header(50, 100, 53000, 4000, 50000, 1);
    // stream = RawStream::new();
}

#[test]
fn test_encode_header() {
    let mut stream: RawStream = Codec::new();
    stream.open_writer("./test_output.addr").expect("Couldn't open file");
    stream.serialize_header(50, 100, 53000, 4000, 50000, 1);
    fs::remove_file("./test_output.addr").unwrap();
}