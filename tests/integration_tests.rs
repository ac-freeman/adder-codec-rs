extern crate adder_codec_rs;

use std::fs;
use std::fs::File;
use std::io::{BufWriter, Write};
use bytes::BytesMut;
use adder_codec_rs::{Codec, Coord, Event};
use adder_codec_rs::raw::raw_stream::RawStream;
use rand::Rng;
use adder_codec_rs::framer::framer::Framer;
use adder_codec_rs::framer::framer::SourceType::U8;

#[test]
#[should_panic]
fn test_encode_header_non_init() {
    let mut stream: RawStream = Codec::new();
    stream.encode_header(50, 100, 53000, 4000, 50000, 1);
    // stream = RawStream::new();
}

#[test]
fn test_encode_header() {
    let n = rand_u32();
    let mut stream = setup_raw_writer(n);
    cleanup_raw_writer(n, &mut stream);
}

fn setup_raw_writer(rand_num: u32) -> RawStream {
    let mut stream: RawStream = Codec::new();
    stream.open_writer("./TEST_".to_owned() + rand_num.to_string().as_str() + ".addr").expect("Couldn't open file");
    stream.encode_header(50, 100, 53000, 4000, 50000, 1);
    stream
}

fn cleanup_raw_writer(rand_num: u32, stream: &mut RawStream) {
    stream.close_writer();
    fs::remove_file("./TEST_".to_owned() + rand_num.to_string().as_str() + ".addr");  // Don't check the error
}

#[test]
#[should_panic]
fn test_encode_bad_event1() {
    let n = 0;
    let mut stream = setup_raw_writer(n);
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
    cleanup_raw_writer(n, &mut stream)
}

#[test]
#[should_panic]
fn test_encode_bad_event2() {
    let n = 0;
    let mut stream = setup_raw_writer(n);
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
    cleanup_raw_writer(n, &mut stream)
}

#[test]
#[should_panic]
fn test_encode_bad_event3() {
    let n = 0;
    let mut stream = setup_raw_writer(n);
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
    cleanup_raw_writer(n, &mut stream)
}

#[test]
fn test_encode_event() {
    let n = 0;
    let mut stream = setup_raw_writer(n);
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
    cleanup_raw_writer(n, &mut stream)
}

#[test]
fn test_encode_events() {
    let n = 0;
    let mut stream = setup_raw_writer(n);
    let event: Event = Event {
        coord: Coord {
            x: 10,
            y: 30,
            c: None
        },
        d: 5,
        delta_t: 1000
    };
    let events = vec![event, event, event];
    stream.encode_events(&events);
    stream.flush_writer();
    cleanup_raw_writer(n, &mut stream)
}

fn setup_raw_reader(rand_num: u32, stream: &mut RawStream) {
    stream.open_reader("./TEST_".to_owned() + rand_num.to_string().as_str() + ".addr").expect("Couldn't open file");
    stream.decode_header();

}

fn rand_u32() -> u32 {
    rand::thread_rng().gen()
}

#[test]
fn read_header() {
    let n: u32 = rand::thread_rng().gen();
    let mut stream = setup_raw_writer(n);
    stream.flush_writer();
    setup_raw_reader(n, &mut stream);
    cleanup_raw_writer(n, &mut stream);
}

#[test]
fn read_event() {
    let n: u32 = rand::thread_rng().gen();
    let mut stream = setup_raw_writer(n);
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
    stream.flush_writer();
    setup_raw_reader(n, &mut stream);
    let res = stream.decode_event();
    match res {
        Ok(decoded_event) => {
            assert_eq!(event, decoded_event);
        }
        Err(_) => {
            panic!("Couldn't decode event")
        }
    }

    cleanup_raw_writer(n, &mut stream);
}

#[test]
fn test_iter_2d() {
    use adder_codec_rs::Event;
    use adder_codec_rs::framer::array3d::{Array3D, Array3DError};
    let mut  arr: Array3D<u16> = Array3D::new(10, 10, 3);
    arr.set_at(100, 0,0,0);
    arr.set_at(250, 0,0,1);
    arr.set_at(325,0,0,2);
    for elem in &arr.iter_2d() {
        let first_sum = elem.sum::<u16>();  // Just summing the first element to show an example
        assert_eq!(first_sum, 675);
        break;
    }
    for mut elem in &arr.iter_2d_mut() {
        for i in elem {
            *i = *i + 1;
        }
        break;
    }
    for elem in &arr.iter_2d() {
        let first_sum = elem.sum::<u16>();
        assert_eq!(first_sum, 678);
        break;
    }
}

#[test]
fn test_event_framer_ingest() {
    use adder_codec_rs::{Coord, Event};
    use adder_codec_rs::framer::framer::FramerMode::INSTANTANEOUS;
    use adder_codec_rs::framer::framer::{FrameSequence, Framer, EventCoordless};
    use adder_codec_rs::framer::framer::SourceType::U8;
    let mut frame_sequence: FrameSequence<Option<EventCoordless>> = FrameSequence::<Option<EventCoordless>>::new(10, 10, 3, 50000, 50, 15, 50000, INSTANTANEOUS, U8);
    let event: Event = Event {
            coord: Coord {
                x: 5,
                y: 5,
                c: Some(1)
            },
            d: 5,
            delta_t: 5000
        };
    let elem = frame_sequence.px_at_frame(5, 5, 1,5);
    assert!(elem.is_none());
    frame_sequence.ingest_event(&event);
    let elem = frame_sequence.px_at_frame(5, 5, 1,5).unwrap();
    let stored_event = elem.unwrap();
    assert!(elem.is_some());
    let elem = frame_sequence.px_at_current(5, 5, 1).unwrap();
    assert!(elem.is_some());

    let event2: Event = Event {
        coord: Coord {
            x: 5,
            y: 5,
            c: Some(1)
        },
        d: 5,
        delta_t: 5100
    };
    frame_sequence.ingest_event(&event2);
    let elem = frame_sequence.px_at_current(5, 5, 1).unwrap();
    assert!(elem.is_some());
    let stored_event2 = elem.unwrap();
    assert_eq!(stored_event2, stored_event);
}

#[test]
fn test_event_framer_ingest_get_filled() {
    use adder_codec_rs::{Coord, Event};
    use adder_codec_rs::framer::framer::FramerMode::INSTANTANEOUS;
    use adder_codec_rs::framer::framer::{FrameSequence, Framer, EventCoordless};
    use adder_codec_rs::framer::framer::SourceType::U8;
    let mut frame_sequence: FrameSequence<Option<EventCoordless>> = FrameSequence::<Option<EventCoordless>>::new(5, 5, 1, 50000, 50, 15, 50000, INSTANTANEOUS, U8);

    for i in 0..5 {
        for j in 0..5{
            let event: Event = Event {
                coord: Coord {
                    x: i,
                    y: j,
                    c: None
                },
                d: 5,
                delta_t: 5100
            };
            let filled = frame_sequence.ingest_event(&event).unwrap();
            if i < 4 || j < 4 {
                assert_eq!(filled, false)
            } else {
                assert_eq!(filled, true)
            }
        }
        if i < 4 {
            assert_eq!(frame_sequence.is_frame_filled(0).unwrap(), false);
        } else {
            assert_eq!(frame_sequence.is_frame_filled(0).unwrap(), true);
        }

    }
}

#[test]
fn get_frame_bytes_eventcoordless() {
    use adder_codec_rs::{Coord, Event};
    use adder_codec_rs::framer::framer::FramerMode::INSTANTANEOUS;
    use adder_codec_rs::framer::framer::{FrameSequence, Framer, EventCoordless};
    use adder_codec_rs::framer::framer::SourceType::U8;
    let mut frame_sequence: FrameSequence<Option<EventCoordless>> = FrameSequence::<Option<EventCoordless>>::new(5, 5, 1, 50000, 50, 15, 50000, INSTANTANEOUS, U8);

    for i in 0..5 {
        for j in 0..5{
            let event: Event = Event {
                coord: Coord {
                    x: i,
                    y: j,
                    c: None
                },
                d: 5,
                delta_t: 5100
            };
            let filled = frame_sequence.ingest_event(&event).unwrap();
            if i < 4 || j < 4 {
                assert_eq!(filled, false)
            } else {
                assert_eq!(filled, true)
            }
        }
        if i < 4 {
            assert_eq!(frame_sequence.is_frame_filled(0).unwrap(), false);
        } else {
            assert_eq!(frame_sequence.is_frame_filled(0).unwrap(), true);
        }

    }
    match frame_sequence.get_frame_bytes() {
        None => {}
        Some(frame_bytes) => {
            let n: u32 = rand::thread_rng().gen();
            let path = "./TEST_".to_owned() + n.to_string().as_str() + ".addr";
            let file = File::create(&path).unwrap();
            let mut output_writer = BufWriter::new(file);
            output_writer.write_all(&*frame_bytes);
            output_writer.flush().unwrap();
            std::mem::drop(output_writer);
            assert_eq!(fs::metadata(&path).unwrap().len(), 125);
        }
    }
}


#[test]
fn get_frame_bytes_u8() {
    use adder_codec_rs::{Coord, Event};
    use adder_codec_rs::framer::framer::FramerMode::INSTANTANEOUS;
    use adder_codec_rs::framer::framer::{FrameSequence, Framer, EventCoordless};
    use adder_codec_rs::framer::framer::SourceType::U8;
    let mut frame_sequence: FrameSequence<u8> = FrameSequence::<u8>::new(5, 5, 1, 50000, 50, 15, 50000, INSTANTANEOUS, U8);

    for i in 0..5 {
        for j in 0..5{
            let event: Event = Event {
                coord: Coord {
                    x: i,
                    y: j,
                    c: None
                },
                d: 5,
                delta_t: 5100
            };
            let filled = frame_sequence.ingest_event(&event).unwrap();
            if i < 4 || j < 4 {
                assert_eq!(filled, false)
            } else {
                assert_eq!(filled, true)
            }
        }
        if i < 4 {
            assert_eq!(frame_sequence.is_frame_filled(0).unwrap(), false);
        } else {
            assert_eq!(frame_sequence.is_frame_filled(0).unwrap(), true);
        }

    }
    match frame_sequence.get_frame_bytes() {
        None => {}
        Some(frame_bytes) => {
            let n: u32 = rand::thread_rng().gen();
            let path = "./TEST_".to_owned() + n.to_string().as_str() + ".addr";
            let file = File::create(&path).unwrap();
            let mut output_writer = BufWriter::new(file);
            output_writer.write_all(&*frame_bytes);
            output_writer.flush().unwrap();
            std::mem::drop(output_writer);
            assert_eq!(fs::metadata(&path).unwrap().len(), 25);
        }
    }
}