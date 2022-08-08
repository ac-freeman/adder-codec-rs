extern crate adder_codec_rs;

use std::fs;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::mem::size_of;
use std::path::Path;
use std::process::Command;
use bytes::BytesMut;
use ndarray::Array3;
use adder_codec_rs::{Codec, Coord, D_MAX, Event};
use adder_codec_rs::raw::raw_stream::RawStream;
use rand::Rng;
use adder_codec_rs::framer::framer::{Framer, FrameSequence};
use adder_codec_rs::framer::framer::FramerMode::{INSTANTANEOUS, INTEGRATION};
use adder_codec_rs::framer::framer::SourceType::U8;


#[test]
fn test_sample_perfect_dt() {
    let input_path = "./tests/samples/sample_1_raw_events.adder";
    let mut stream: RawStream = Codec::new();
    stream.open_reader(input_path.to_string());
    stream.decode_header();

    let output_path =  Path::new("./tests/samples/temp_sample_1");
    let mut output_stream = BufWriter::new(File::create(output_path).unwrap());

    let reconstructed_frame_rate = 24;
    // For instantaneous reconstruction, make sure the frame rate matches the source video rate
    assert_eq!(stream.tps / stream.ref_interval, reconstructed_frame_rate);

    let mut frame_sequence: FrameSequence<u8> = FrameSequence::<u8>::new(stream.height.into(), stream.width.into(), stream.channels.into(), stream.tps, reconstructed_frame_rate, D_MAX, stream.delta_t_max, INSTANTANEOUS, U8);
    let mut frame_count = 0;
    loop {
        match stream.decode_event() {
            Ok(event) => {
                if frame_sequence.ingest_event(&event).unwrap() {
                    match frame_sequence.write_multi_frame_bytes(&mut output_stream) {
                        0 => { panic!("should have frame") },
                        frames_returned => {
                            frame_count += frames_returned;
                        }
                    }
                }



            }
            Err(e) => {
                eprintln!("\nExiting");
                break
            }
        }
    }

    output_stream.flush().unwrap();
    let output = if !cfg!(target_os = "windows") {
        Command::new("sh")
            .arg("-c")
            .arg("cmp ./temp_sample_1 ./sample_1_instant_framed.gray")
            .output()
            .expect("failed to execute process")
    } else {
        fs::remove_file(output_path);
        return;
    };
    assert_eq!(output.stdout.len(), 0);
    fs::remove_file(output_path);
}

#[test]
fn test_sample_perfect_dt_color() {
    let input_path = "./tests/samples/sample_2_raw_events.adder";
    let mut stream: RawStream = Codec::new();
    stream.open_reader(input_path.to_string());
    stream.decode_header();

    let output_path =  Path::new("./tests/samples/temp_sample_2");
    let mut output_stream = BufWriter::new(File::create(output_path).unwrap());

    let reconstructed_frame_rate = 24;
    // For instantaneous reconstruction, make sure the frame rate matches the source video rate
    assert_eq!(stream.tps / stream.ref_interval, reconstructed_frame_rate);

    let mut frame_sequence: FrameSequence<u8> = FrameSequence::<u8>::new(stream.height.into(), stream.width.into(), stream.channels.into(), stream.tps, reconstructed_frame_rate, D_MAX, stream.delta_t_max, INSTANTANEOUS, U8);
    let mut frame_count = 0;
    loop {
        match stream.decode_event() {
            Ok(event) => {
                if frame_sequence.ingest_event(&event).unwrap() {
                    match frame_sequence.write_multi_frame_bytes(&mut output_stream) {
                        0 => { panic!("should have frame") },
                        frames_returned => {
                            frame_count += frames_returned;
                        }
                    }
                }



            }
            Err(e) => {
                eprintln!("\nExiting");
                break
            }
        }
    }

    output_stream.flush().unwrap();
    let output = if !cfg!(target_os = "windows") {
        Command::new("sh")
            .arg("-c")
            .arg("cmp ./temp_sample_1 ./sample_2_instant_framed.bgr24")
            .output()
            .expect("failed to execute process")
    } else {
        fs::remove_file(output_path);
        return;
    };
    assert_eq!(output.stdout.len(), 0);
    fs::remove_file(output_path);
}

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
    stream.close_writer();
    assert_eq!(fs::metadata("./TEST_".to_owned() + n.to_string().as_str() + ".addr").unwrap().len(), 35);
    fs::remove_file("./TEST_".to_owned() + n.to_string().as_str() + ".addr");  // Don't check the error
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
fn test_event_framer_ingest() {
    use adder_codec_rs::{Coord, Event};
    use adder_codec_rs::framer::framer::FramerMode::INSTANTANEOUS;
    use adder_codec_rs::framer::framer::{FrameSequence, Framer, EventCoordless};
    use adder_codec_rs::framer::framer::SourceType::U8;
    use std::fs::File;
    use std::io;
    use std::io::{BufWriter, Write};
    use std::time::Instant;
    use adder_codec_rs::{Codec, D_MAX};
    use adder_codec_rs::framer::array3d::Array3DError;
    use adder_codec_rs::raw::raw_stream::RawStream;
    let mut frame_sequence: FrameSequence<EventCoordless> = FrameSequence::<EventCoordless>::new(10, 10, 3, 50000, 50, 15, 50000, INSTANTANEOUS, U8);
    let event: Event = Event {
            coord: Coord {
                x: 5,
                y: 5,
                c: Some(1)
            },
            d: 5,
            delta_t: 5000
        };
    frame_sequence.ingest_event(&event);

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
}

#[test]
fn test_event_framer_ingest_get_filled() {
    use adder_codec_rs::{Coord, Event};
    use adder_codec_rs::framer::framer::FramerMode::INSTANTANEOUS;
    use adder_codec_rs::framer::framer::{FrameSequence, Framer, EventCoordless};
    use adder_codec_rs::framer::framer::SourceType::U8;
    let mut frame_sequence: FrameSequence<EventCoordless> = FrameSequence::<EventCoordless>::new(5, 5, 1, 50000, 50, 15, 50000, INSTANTANEOUS, U8);

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
    let mut frame_sequence: FrameSequence<EventCoordless> = FrameSequence::<EventCoordless>::new(5, 5, 1, 50000, 1, 15, 50000, INSTANTANEOUS, U8);
    eprintln!("{}", std::mem::size_of::<EventCoordless>());
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
    let n: u32 = rand::thread_rng().gen();
    let path = "./TESTttttt_".to_owned() + n.to_string().as_str() + ".addr";
    let file = File::create(&path).unwrap();
    let mut output_writer = BufWriter::new(file);

    assert_eq!(fs::metadata(&path).unwrap().len(), 0);
    match frame_sequence.write_multi_frame_bytes(&mut output_writer) {
        frame_count if frame_count == 1 => {
            output_writer.flush().unwrap();
            std::mem::drop(output_writer);

            // No header. 8 bytes per eventcoordless *
            assert_eq!(fs::metadata(&path).unwrap().len(), 125);
            fs::remove_file(&path);  // Don't check the error
        }
        _ => {
            panic!("fail")
        }
    }
}


// #[test]
// fn get_frame_bytes_u8() {
//     use adder_codec_rs::{Coord, Event};
//     use adder_codec_rs::framer::framer::FramerMode::INSTANTANEOUS;
//     use adder_codec_rs::framer::framer::{FrameSequence, Framer, EventCoordless};
//     use adder_codec_rs::framer::framer::SourceType::U8;
//     let mut frame_sequence: FrameSequence<u8> = FrameSequence::<u8>::new(5, 5, 1, 50000, 50, 15, 50000, INSTANTANEOUS, U8);
//
//     for i in 0..5 {
//         for j in 0..5{
//             let event: Event = Event {
//                 coord: Coord {
//                     x: i,
//                     y: j,
//                     c: None
//                 },
//                 d: 5,
//                 delta_t: 5100
//             };
//             let filled = frame_sequence.ingest_event(&event).unwrap();
//             if i < 4 || j < 4 {
//                 assert_eq!(filled, false)
//             } else {
//                 assert_eq!(filled, true)
//             }
//         }
//         if i < 4 {
//             assert_eq!(frame_sequence.is_frame_filled(0).unwrap(), false);
//         } else {
//             assert_eq!(frame_sequence.is_frame_filled(0).unwrap(), true);
//         }
//
//     }
//     match frame_sequence.get_frame_bytes() {
//         None => {}
//         Some(frame_bytes) => {
//             let n: u32 = rand::thread_rng().gen();
//             let path = "./TEST_".to_owned() + n.to_string().as_str() + ".addr";
//             let file = File::create(&path).unwrap();
//             let mut output_writer = BufWriter::new(file);
//             output_writer.write_all(&*frame_bytes);
//             output_writer.flush().unwrap();
//             std::mem::drop(output_writer);
//             assert_eq!(fs::metadata(&path).unwrap().len(), 25);
//             fs::remove_file(&path);  // Don't check the error
//
//         }
//     }
// }
//
// #[test]
// fn get_frame_bytes_u16() {
//     use adder_codec_rs::{Coord, Event};
//     use adder_codec_rs::framer::framer::FramerMode::INSTANTANEOUS;
//     use adder_codec_rs::framer::framer::{FrameSequence, Framer, EventCoordless};
//     use adder_codec_rs::framer::framer::SourceType::U8;
//     let mut frame_sequence: FrameSequence<u16> = FrameSequence::<u16>::new(5, 5, 1, 50000, 50, 15, 50000, INSTANTANEOUS, U8);
//
//     for i in 0..5 {
//         for j in 0..5{
//             let event: Event = Event {
//                 coord: Coord {
//                     x: i,
//                     y: j,
//                     c: None
//                 },
//                 d: 5,
//                 delta_t: 5100
//             };
//             let filled = frame_sequence.ingest_event(&event).unwrap();
//             if i < 4 || j < 4 {
//                 assert_eq!(filled, false)
//             } else {
//                 assert_eq!(filled, true)
//             }
//         }
//         if i < 4 {
//             assert_eq!(frame_sequence.is_frame_filled(0).unwrap(), false);
//         } else {
//             assert_eq!(frame_sequence.is_frame_filled(0).unwrap(), true);
//         }
//
//     }
//     match frame_sequence.get_frame_bytes() {
//         None => {}
//         Some(frame_bytes) => {
//             let n: u32 = rand::thread_rng().gen();
//             let path = "./TEST_".to_owned() + n.to_string().as_str() + ".addr";
//             let file = File::create(&path).unwrap();
//             let mut output_writer = BufWriter::new(file);
//             output_writer.write_all(&*frame_bytes);
//             output_writer.flush().unwrap();
//             std::mem::drop(output_writer);
//             assert_eq!(fs::metadata(&path).unwrap().len(), 50);
//             fs::remove_file(&path);  // Don't check the error
//         }
//     }
// }
//
// #[test]
// fn get_frame_bytes_u32() {
//     use adder_codec_rs::{Coord, Event};
//     use adder_codec_rs::framer::framer::FramerMode::INSTANTANEOUS;
//     use adder_codec_rs::framer::framer::{FrameSequence, Framer, EventCoordless};
//     use adder_codec_rs::framer::framer::SourceType::U8;
//     let mut frame_sequence: FrameSequence<u32> = FrameSequence::<u32>::new(5, 5, 1, 50000, 50, 15, 50000, INSTANTANEOUS, U8);
//
//     for i in 0..5 {
//         for j in 0..5{
//             let event: Event = Event {
//                 coord: Coord {
//                     x: i,
//                     y: j,
//                     c: None
//                 },
//                 d: 5,
//                 delta_t: 5100
//             };
//             let filled = frame_sequence.ingest_event(&event).unwrap();
//             if i < 4 || j < 4 {
//                 assert_eq!(filled, false)
//             } else {
//                 assert_eq!(filled, true)
//             }
//         }
//         if i < 4 {
//             assert_eq!(frame_sequence.is_frame_filled(0).unwrap(), false);
//         } else {
//             assert_eq!(frame_sequence.is_frame_filled(0).unwrap(), true);
//         }
//
//     }
//     match frame_sequence.get_frame_bytes() {
//         None => {}
//         Some(frame_bytes) => {
//             let n: u32 = rand::thread_rng().gen();
//             let path = "./TEST_".to_owned() + n.to_string().as_str() + ".addr";
//             let file = File::create(&path).unwrap();
//             let mut output_writer = BufWriter::new(file);
//             output_writer.write_all(&*frame_bytes);
//             output_writer.flush().unwrap();
//             std::mem::drop(output_writer);
//             assert_eq!(fs::metadata(&path).unwrap().len(), 100);
//             fs::remove_file(&path);  // Don't check the error
//         }
//     }
// }
//
// #[test]
// fn get_frame_bytes_u64() {
//     use adder_codec_rs::{Coord, Event};
//     use adder_codec_rs::framer::framer::FramerMode::INSTANTANEOUS;
//     use adder_codec_rs::framer::framer::{FrameSequence, Framer, EventCoordless};
//     use adder_codec_rs::framer::framer::SourceType::U8;
//     let mut frame_sequence: FrameSequence<u64> = FrameSequence::<u64>::new(5, 5, 1, 50000, 50, 15, 50000, INSTANTANEOUS, U8);
//
//     for i in 0..5 {
//         for j in 0..5{
//             let event: Event = Event {
//                 coord: Coord {
//                     x: i,
//                     y: j,
//                     c: None
//                 },
//                 d: 5,
//                 delta_t: 5100
//             };
//             let filled = frame_sequence.ingest_event(&event).unwrap();
//             if i < 4 || j < 4 {
//                 assert_eq!(filled, false)
//             } else {
//                 assert_eq!(filled, true)
//             }
//         }
//         if i < 4 {
//             assert_eq!(frame_sequence.is_frame_filled(0).unwrap(), false);
//         } else {
//             assert_eq!(frame_sequence.is_frame_filled(0).unwrap(), true);
//         }
//
//     }
//     match frame_sequence.get_frame_bytes() {
//         None => {}
//         Some(frame_bytes) => {
//             let n: u32 = rand::thread_rng().gen();
//             let path = "./TEST_".to_owned() + n.to_string().as_str() + ".addr";
//             let file = File::create(&path).unwrap();
//             let mut output_writer = BufWriter::new(file);
//             output_writer.write_all(&*frame_bytes);
//             output_writer.flush().unwrap();
//             std::mem::drop(output_writer);
//             assert_eq!(fs::metadata(&path).unwrap().len(), 200);
//             fs::remove_file(&path);  // Don't check the error
//         }
//     }
// }
//
// #[test]
// fn test_get_empty_frame() {
//     use adder_codec_rs::{Coord, Event};
//     use adder_codec_rs::framer::framer::FramerMode::INSTANTANEOUS;
//     use adder_codec_rs::framer::framer::{FrameSequence, Framer, EventCoordless};
//     use adder_codec_rs::framer::framer::SourceType::U8;
//     let mut frame_sequence: FrameSequence<u8> = FrameSequence::<u8>::new(5, 5, 1, 50000, 50, 15, 50000, INSTANTANEOUS, U8);
//     assert!(frame_sequence.get_frame_bytes().is_some()); // Even if it's all empty data, still want
//     // to return it as a frame. Up to the user to make sure that the frame is filled.
//     let event: Event = Event {
//         coord: Coord {
//             x: 0,
//             y: 0,
//             c: None
//         },
//         d: 5,
//         delta_t: 500
//     };
//     let filled = frame_sequence.ingest_event(&event).unwrap();
//     assert_eq!(filled, false);
// }
//
// #[test]
// fn get_frame_bytes_u8_integration() {
//     use adder_codec_rs::{Coord, Event};
//     use adder_codec_rs::framer::framer::FramerMode::INSTANTANEOUS;
//     use adder_codec_rs::framer::framer::{FrameSequence, Framer, EventCoordless};
//     use adder_codec_rs::framer::framer::SourceType::U8;
//     let mut frame_sequence: FrameSequence<u8> = FrameSequence::<u8>::new(5, 5, 1, 50000, 50, 15, 50000, INTEGRATION, U8);
//
//     for i in 0..5 {
//         for j in 0..5{
//             let event: Event = Event {
//                 coord: Coord {
//                     x: i,
//                     y: j,
//                     c: None
//                 },
//                 d: 5,
//                 delta_t: 5100
//             };
//             let filled = frame_sequence.ingest_event(&event).unwrap();
//             if i < 4 || j < 4 {
//                 assert_eq!(filled, false)
//             } else {
//                 assert_eq!(filled, true)
//             }
//         }
//         if i < 4 {
//             assert_eq!(frame_sequence.is_frame_filled(0).unwrap(), false);
//         } else {
//             assert_eq!(frame_sequence.is_frame_filled(0).unwrap(), true);
//         }
//
//     }
//     match frame_sequence.get_frame_bytes() {
//         None => {}
//         Some(frame_bytes) => {
//             let n: u32 = rand::thread_rng().gen();
//             let path = "./TEST_".to_owned() + n.to_string().as_str() + ".addr";
//             let file = File::create(&path).unwrap();
//             let mut output_writer = BufWriter::new(file);
//             output_writer.write_all(&*frame_bytes);
//             output_writer.flush().unwrap();
//             std::mem::drop(output_writer);
//             assert_eq!(fs::metadata(&path).unwrap().len(), 25);
//             fs::remove_file(&path);  // Don't check the error
//         }
//     }
// }