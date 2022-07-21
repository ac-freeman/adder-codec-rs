extern crate adder_codec_rs;

use std::fs;
use adder_codec_rs::{Codec, Coord, Event};
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
    let mut stream = setup_raw_writer();
    cleanup_raw_writer(&mut stream);
}

fn setup_raw_writer() -> RawStream {
    let mut stream: RawStream = Codec::new();
    stream.open_writer("./test_output.addr").expect("Couldn't open file");
    stream.serialize_header(50, 100, 53000, 4000, 50000, 1);
    stream
}

fn cleanup_raw_writer(stream: &mut RawStream) {
    stream.close_writer();
    fs::remove_file("./test_output.addr");  // Don't check the error
}

#[test]
fn test_encode_event() {
    let mut stream = setup_raw_writer();
    let event: Event = Event {
        coord: Coord {
            x: 10,
            y: 30,
            c: None
        },
        d: 5,
        delta_t: 1000
    };
    stream.encode_event(&event);
    cleanup_raw_writer(&mut stream)
}

#[test]
#[should_panic]
fn test_encode_bad_event1() {
    let mut stream = setup_raw_writer();
    let event: Event = Event {
        coord: Coord {
            x: 100,
            y: 30,
            c: None
        },
        d: 5,
        delta_t: 1000
    };
    stream.encode_event(&event);
    cleanup_raw_writer(&mut stream)
}

#[test]
#[should_panic]
fn test_encode_bad_event2() {
    let mut stream = setup_raw_writer();
    let event: Event = Event {
        coord: Coord {
            x: 10,
            y: 30,
            c: Some(1)
        },
        d: 5,
        delta_t: 1000
    };
    stream.encode_event(&event);
    cleanup_raw_writer(&mut stream)
}

#[test]
#[should_panic]
fn test_encode_bad_event3() {
    let mut stream = setup_raw_writer();
    let event: Event = Event {
        coord: Coord {
            x: 10,
            y: 30,
            c: None
        },
        d: 5,
        delta_t: 1000000
    };
    stream.encode_event(&event);
    cleanup_raw_writer(&mut stream)
}