extern crate adder_codec_rs;

use crate::adder_codec_rs::transcoder::source::video::VideoBuilder;
use adder_codec_core::codec::decoder::Decoder;
use adder_codec_core::codec::encoder::Encoder;
use adder_codec_core::codec::raw::stream::RawInput;
use adder_codec_core::codec::{ReadCompression, WriteCompression};
use adder_codec_core::SourceCamera::FramedU8;
use adder_codec_core::SourceType::*;
use adder_codec_core::TimeMode::DeltaT;
use adder_codec_core::{Coord, Event, EventCoordless, PlaneSize, TimeMode};
use bitstream_io::{BigEndian, BitReader};
use ndarray::{Array3, Axis};
use std::fs;
use std::fs::File;
use std::io::{BufReader, BufWriter, Write};
use std::path::Path;
use std::process::Command;

use adder_codec_rs::framer::driver::FramerMode::INSTANTANEOUS;
use adder_codec_rs::framer::driver::{FrameSequence, Framer, FramerBuilder};
use adder_codec_rs::transcoder::source::framed::Framed;
use adder_codec_rs::transcoder::source::video::Source;
use rand::Rng;
use rayon::current_num_threads;

#[test]
fn test_set_stream_position() {
    let input_path = "./tests/samples/sample_1_raw_events.adder";
    let tmp = File::open(input_path).unwrap();
    let bufreader = BufReader::new(tmp);
    let compression = RawInput::new();

    let mut bitreader = BitReader::endian(bufreader, BigEndian);
    let mut reader = Decoder::new_raw(compression, &mut bitreader).unwrap();
    for i in 1..reader.meta().event_size as usize {
        assert!(reader
            .set_input_stream_position(&mut bitreader, (reader.meta().header_size + i) as u64)
            .is_err());
    }

    assert!(reader
        .set_input_stream_position(
            &mut bitreader,
            (reader.meta().header_size + reader.meta().event_size as usize) as u64
        )
        .is_ok());
    assert!(reader
        .set_input_stream_position(
            &mut bitreader,
            (reader.meta().header_size + reader.meta().event_size as usize * 2) as u64
        )
        .is_ok());
}

#[test]
fn test_sample_perfect_dt() {
    let input_path = "./tests/samples/sample_1_raw_events.adder";
    let tmp = File::open(input_path).unwrap();
    let bufreader = BufReader::new(tmp);
    let compression = RawInput::new();

    let mut bitreader = BitReader::endian(bufreader, BigEndian);
    let mut reader = Decoder::new_raw(compression, &mut bitreader).unwrap();

    let output_path = Path::new("./tests/samples/temp_sample_1");
    let mut output_stream = BufWriter::new(File::create(output_path).unwrap());

    let reconstructed_frame_rate = 24.0;
    // For instantaneous reconstruction, make sure the frame rate matches the source video rate
    assert_eq!(
        reader.meta().tps / reader.meta().ref_interval,
        reconstructed_frame_rate as u32
    );

    let mut frame_sequence: FrameSequence<u8> = FramerBuilder::new(reader.meta().plane, 64)
        .codec_version(reader.meta().codec_version, TimeMode::DeltaT)
        .time_parameters(
            reader.meta().tps,
            reader.meta().ref_interval,
            reader.meta().delta_t_max,
            reconstructed_frame_rate,
        )
        .mode(INSTANTANEOUS)
        .source(reader.get_source_type(), reader.meta().source_camera)
        .finish();

    let mut frame_count = 0;
    loop {
        match reader.digest_event(&mut bitreader) {
            Ok(mut event) => {
                if frame_sequence.ingest_event(&mut event) {
                    match frame_sequence.write_multi_frame_bytes(&mut output_stream) {
                        Ok(0) => {
                            panic!("should have frame")
                        }
                        Ok(frames_returned) => {
                            frame_count += frames_returned;
                        }
                        Err(e) => {
                            panic!("error writing frame: {e}")
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

    assert_eq!(frame_count, 221);

    output_stream.flush().unwrap();
    let output = if !cfg!(target_os = "windows") {
        Command::new("sh")
            .arg("-c")
            .arg("cmp ./temp_sample_1 ./sample_1_instant_framed.gray")
            .output()
            .expect("failed to execute process")
    } else {
        fs::remove_file(output_path).unwrap();
        return;
    };
    assert_eq!(output.stdout.len(), 0);
    fs::remove_file(output_path).unwrap();
}

#[test]
fn test_sample_perfect_dt_color() {
    let input_path = "./tests/samples/sample_2_raw_events.adder";
    let tmp = File::open(input_path).unwrap();
    let bufreader = BufReader::new(tmp);
    let compression = RawInput::new();

    let mut bitreader = BitReader::endian(bufreader, BigEndian);
    let mut reader = Decoder::new_raw(compression, &mut bitreader).unwrap();

    let output_path = Path::new("./tests/samples/temp_sample_2");
    let mut output_stream = BufWriter::new(File::create(output_path).unwrap());

    let reconstructed_frame_rate = 24.0;
    // For instantaneous reconstruction, make sure the frame rate matches the source video rate
    assert_eq!(
        reader.meta().tps / reader.meta().ref_interval,
        reconstructed_frame_rate as u32
    );

    let mut frame_sequence: FrameSequence<u8> = FramerBuilder::new(reader.meta().plane, 64)
        .codec_version(reader.meta().codec_version, TimeMode::DeltaT)
        .time_parameters(
            reader.meta().tps,
            reader.meta().ref_interval,
            reader.meta().delta_t_max,
            reconstructed_frame_rate,
        )
        .mode(INSTANTANEOUS)
        .source(reader.get_source_type(), reader.meta().source_camera)
        .finish();

    let mut frame_count = 0;
    loop {
        match reader.digest_event(&mut bitreader) {
            Ok(mut event) => {
                if frame_sequence.ingest_event(&mut event) {
                    match frame_sequence.write_multi_frame_bytes(&mut output_stream) {
                        Ok(0) => {
                            panic!("should have frame")
                        }
                        Ok(frames_returned) => {
                            frame_count += frames_returned;
                        }
                        Err(e) => {
                            panic!("error writing frame: {e}")
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
    let output = if !cfg!(target_os = "windows") {
        Command::new("sh")
            .arg("-c")
            .arg("cmp ./temp_sample_1 ./sample_2_instant_framed.bgr24")
            .output()
            .expect("failed to execute process")
    } else {
        fs::remove_file(output_path).unwrap();
        return;
    };
    assert_eq!(frame_count, 221);
    assert_eq!(output.stdout.len(), 0);
    fs::remove_file(output_path).unwrap();
}

#[test]
fn test_encode_header_v0() {
    let n = rand_u32();
    let stream = setup_raw_writer_v0(n);
    stream.close_writer().unwrap();
    assert_eq!(
        fs::metadata("./TEST_".to_owned() + n.to_string().as_str() + ".addr")
            .unwrap()
            .len(),
        36
    );
    fs::remove_file("./TEST_".to_owned() + n.to_string().as_str() + ".addr").unwrap();
    // Don't check the error
}

#[test]
fn test_encode_header_v1() {
    let n = rand_u32();
    let mut stream = setup_raw_writer_v1(n);
    stream.flush_writer().unwrap();
    assert_eq!(stream.meta().header_size, 29);
    stream.close_writer().unwrap();
    assert_eq!(
        fs::metadata("./TEST_".to_owned() + n.to_string().as_str() + ".addr")
            .unwrap()
            .len(),
        40
    );
    fs::remove_file("./TEST_".to_owned() + n.to_string().as_str() + ".addr").unwrap();
    // Don't check the error
}

#[test]
fn test_encode_header_v2() {
    let n: u32 = rand::thread_rng().gen();
    let mut stream = setup_raw_writer_v2(n);
    stream.flush_writer().unwrap();
    assert_eq!(stream.meta().header_size, 33);
    assert_eq!(
        fs::metadata("./TEST_".to_owned() + n.to_string().as_str() + ".addr")
            .unwrap()
            .len(),
        33
    );
    fs::remove_file("./TEST_".to_owned() + n.to_string().as_str() + ".addr").unwrap();
}

fn setup_raw_writer_v0(rand_num: u32) -> Encoder<BufWriter<File>> {
    let plane = PlaneSize::new(50, 100, 1).unwrap();

    let bufwriter = BufWriter::new(
        File::create("./TEST_".to_owned() + rand_num.to_string().as_str() + ".addr").unwrap(),
    );
    let compression = adder_codec_core::codec::raw::stream::RawOutput::new(
        adder_codec_core::codec::CodecMetadata {
            codec_version: 0,
            header_size: 0,
            time_mode: DeltaT,
            plane,
            tps: 53000,
            ref_interval: 4000,
            delta_t_max: 50000,
            event_size: 0,
            source_camera: Default::default(),
        },
        bufwriter,
    );
    let encoder: Encoder<BufWriter<File>> = Encoder::new_raw(compression);
    encoder
}

fn setup_raw_writer_v1(rand_num: u32) -> Encoder<BufWriter<File>> {
    let plane = PlaneSize::new(50, 100, 1).unwrap();

    let bufwriter = BufWriter::new(
        File::create("./TEST_".to_owned() + rand_num.to_string().as_str() + ".addr").unwrap(),
    );
    let compression = adder_codec_core::codec::raw::stream::RawOutput::new(
        adder_codec_core::codec::CodecMetadata {
            codec_version: 1,
            header_size: 0,
            time_mode: DeltaT,
            plane,
            tps: 53000,
            ref_interval: 4000,
            delta_t_max: 50000,
            event_size: 0,
            source_camera: FramedU8,
        },
        bufwriter,
    );
    let encoder: Encoder<BufWriter<File>> = Encoder::new_raw(compression);
    encoder
}

fn setup_raw_writer_v2(rand_num: u32) -> Encoder<BufWriter<File>> {
    let plane = PlaneSize::new(50, 100, 1).unwrap();

    let bufwriter = BufWriter::new(
        File::create("./TEST_".to_owned() + rand_num.to_string().as_str() + ".addr").unwrap(),
    );
    let compression = adder_codec_core::codec::raw::stream::RawOutput::new(
        adder_codec_core::codec::CodecMetadata {
            codec_version: 2,
            header_size: 0,
            time_mode: DeltaT,
            plane,
            tps: 53000,
            ref_interval: 4000,
            delta_t_max: 50000,
            event_size: 0,
            source_camera: FramedU8,
        },
        bufwriter,
    );
    let encoder: Encoder<BufWriter<File>> = Encoder::new_raw(compression);
    encoder
}

fn cleanup_raw_writer(rand_num: u32, stream: Encoder<BufWriter<File>>) {
    stream.close_writer().unwrap();
    fs::remove_file("./TEST_".to_owned() + rand_num.to_string().as_str() + ".addr").unwrap();
    // Don't check the error
}

#[test]
fn test_encode_event() {
    let n = rand_u32();
    let mut stream = setup_raw_writer_v0(n);
    let event: Event = Event {
        coord: Coord {
            x: 10,
            y: 30,
            c: None,
        },
        d: 5,
        delta_t: 1000,
    };
    stream.ingest_event(event).unwrap();
    cleanup_raw_writer(n, stream)
}

#[test]
fn test_encode_events() {
    let n = rand_u32();
    let mut stream = setup_raw_writer_v0(n);
    let event: Event = Event {
        coord: Coord {
            x: 10,
            y: 30,
            c: None,
        },
        d: 5,
        delta_t: 1000,
    };
    let events = vec![event, event, event];
    stream.ingest_events(&events).unwrap();
    stream.flush_writer().unwrap();
    cleanup_raw_writer(n, stream)
}

fn setup_raw_reader(
    rand_num: u32,
) -> (
    Decoder<BufReader<File>>,
    BitReader<BufReader<File>, BigEndian>,
) {
    let tmp = File::open("./TEST_".to_owned() + rand_num.to_string().as_str() + ".addr").unwrap();
    let bufreader = BufReader::new(tmp);
    let compression = RawInput::new();

    let mut bitreader = BitReader::endian(bufreader, BigEndian);
    let reader = Decoder::new_raw(compression, &mut bitreader).unwrap();
    (reader, bitreader)
}

fn rand_u32() -> u32 {
    rand::thread_rng().gen()
}

#[test]
fn read_header() {
    let n: u32 = rand::thread_rng().gen();
    let stream = setup_raw_writer_v0(n);
    stream.close_writer().unwrap();
    setup_raw_reader(n);
}

#[test]
fn read_event() {
    let n: u32 = rand::thread_rng().gen();
    let mut stream = setup_raw_writer_v0(n);
    let event: Event = Event {
        coord: Coord {
            x: 10,
            y: 30,
            c: None,
        },
        d: 5,
        delta_t: 1000,
    };
    stream.ingest_event(event).unwrap();
    stream.flush_writer().unwrap();
    let (mut reader, mut bitreader) = setup_raw_reader(n);
    let res = reader.digest_event(&mut bitreader);
    match res {
        Ok(decoded_event) => {
            assert_eq!(event, decoded_event);
        }
        Err(_) => {
            panic!("Couldn't decode event")
        }
    }

    cleanup_raw_writer(n, stream);
}

#[test]
fn test_event_framer_ingest() {
    use adder_codec_core::SourceType::U8;
    use adder_codec_rs::framer::driver::FramerMode::INSTANTANEOUS;
    use adder_codec_rs::framer::driver::{FrameSequence, Framer};

    let plane = PlaneSize::new(10, 10, 3).unwrap();
    let mut frame_sequence: FrameSequence<EventCoordless> = FramerBuilder::new(plane, 64)
        .codec_version(1, TimeMode::DeltaT)
        .time_parameters(50000, 1000, 1000, 50.0)
        .mode(INSTANTANEOUS)
        .source(U8, FramedU8)
        .finish();
    let mut event: Event = Event {
        coord: Coord {
            x: 5,
            y: 5,
            c: Some(1),
        },
        d: 5,
        delta_t: 5000,
    };
    frame_sequence.ingest_event(&mut event);

    let mut event2: Event = Event {
        coord: Coord {
            x: 5,
            y: 5,
            c: Some(1),
        },
        d: 5,
        delta_t: 5100,
    };
    frame_sequence.ingest_event(&mut event2);
}

#[test]
fn test_event_framer_ingest_get_filled() {
    use adder_codec_rs::framer::driver::FramerMode::INSTANTANEOUS;
    use adder_codec_rs::framer::driver::{FrameSequence, Framer};
    let plane = PlaneSize::new(5, 5, 1).unwrap();
    let mut frame_sequence: FrameSequence<EventCoordless> = FramerBuilder::new(plane, 64)
        .codec_version(1, TimeMode::DeltaT)
        .time_parameters(50000, 1000, 1000, 50.0)
        .mode(INSTANTANEOUS)
        .source(U8, FramedU8)
        .finish();

    for i in 0..5 {
        for j in 0..5 {
            let mut event: Event = Event {
                coord: Coord {
                    x: i,
                    y: j,
                    c: None,
                },
                d: 5,
                delta_t: 5100,
            };
            let filled = frame_sequence.ingest_event(&mut event);
            if i < 4 || j < 4 {
                assert!(!filled)
            } else {
                assert!(filled)
            }
        }
        if i < 4 {
            assert!(!frame_sequence.is_frame_filled(0).unwrap());
        } else {
            assert!(frame_sequence.is_frame_filled(0).unwrap());
        }
    }
}

#[test]
fn get_frame_bytes_eventcoordless() {
    use adder_codec_rs::framer::driver::FramerMode::INSTANTANEOUS;
    use adder_codec_rs::framer::driver::{FrameSequence, Framer};
    let plane = PlaneSize::new(5, 5, 1).unwrap();
    let mut frame_sequence: FrameSequence<EventCoordless> = FramerBuilder::new(plane, 64)
        .codec_version(1, TimeMode::DeltaT)
        .time_parameters(50000, 1000, 1000, 50.0)
        .mode(INSTANTANEOUS)
        .source(U8, FramedU8)
        .finish();
    eprintln!("{}", std::mem::size_of::<Option<EventCoordless>>());
    for i in 0..5 {
        for j in 0..5 {
            let mut event: Event = Event {
                coord: Coord {
                    x: i,
                    y: j,
                    c: None,
                },
                d: 5,
                delta_t: 5100,
            };
            let filled = frame_sequence.ingest_event(&mut event);
            if i < 4 || j < 4 {
                assert!(!filled)
            } else {
                assert!(filled)
            }
        }
        if i < 4 {
            assert!(!frame_sequence.is_frame_filled(0).unwrap());
        } else {
            assert!(frame_sequence.is_frame_filled(0).unwrap());
        }
    }
    let n: u32 = rand::thread_rng().gen();
    let path = "./TEST_".to_owned() + n.to_string().as_str() + ".addr";
    let file = File::create(&path).unwrap();
    let mut output_writer = BufWriter::new(file);

    assert_eq!(fs::metadata(&path).unwrap().len(), 0);
    match frame_sequence.write_multi_frame_bytes(&mut output_writer) {
        Ok(6) => {
            output_writer.flush().unwrap();
            drop(output_writer);

            // No header. 5 bytes per eventcoordless * 6 frames = 750 bytes
            // TODO: need to serialize just the eventcoordless within, not the Option or the Array3
            assert_eq!(fs::metadata(&path).unwrap().len(), 750);
            fs::remove_file(&path).unwrap(); // Don't check the error
        }
        _ => {
            panic!("fail")
        }
    }
}

#[test]
fn get_frame_bytes_u8() {
    use adder_codec_rs::framer::driver::FramerMode::INSTANTANEOUS;
    use adder_codec_rs::framer::driver::{FrameSequence, Framer};
    let plane = PlaneSize::new(5, 5, 1).unwrap();
    let mut frame_sequence: FrameSequence<u8> = FramerBuilder::new(plane, 64)
        .codec_version(1, TimeMode::DeltaT)
        .time_parameters(50000, 1000, 1000, 50.0)
        .mode(INSTANTANEOUS)
        .source(U8, FramedU8)
        .finish();

    for i in 0..5 {
        for j in 0..5 {
            let mut event: Event = Event {
                coord: Coord {
                    x: i,
                    y: j,
                    c: None,
                },
                d: 5,
                delta_t: 5100,
            };
            let filled = frame_sequence.ingest_event(&mut event);
            if i < 4 || j < 4 {
                assert!(!filled)
            } else {
                assert!(filled)
            }
        }
        if i < 4 {
            assert!(!frame_sequence.is_frame_filled(0).unwrap());
        } else {
            assert!(frame_sequence.is_frame_filled(0).unwrap());
        }
    }

    let n: u32 = rand::thread_rng().gen();
    let path = "./TEST_".to_owned() + n.to_string().as_str() + ".addr";
    let file = File::create(&path).unwrap();
    let mut output_writer = BufWriter::new(file);

    assert_eq!(fs::metadata(&path).unwrap().len(), 0);
    match frame_sequence.write_multi_frame_bytes(&mut output_writer) {
        Ok(6) => {
            output_writer.flush().unwrap();
            drop(output_writer);

            assert_eq!(fs::metadata(&path).unwrap().len(), 150);
            fs::remove_file(&path).unwrap(); // Don't check the error
        }
        _ => {
            panic!("fail")
        }
    }
}

#[test]
fn get_frame_bytes_u16() {
    use adder_codec_rs::framer::driver::FramerMode::INSTANTANEOUS;
    use adder_codec_rs::framer::driver::{FrameSequence, Framer};
    let plane = PlaneSize::new(5, 5, 1).unwrap();
    let mut frame_sequence: FrameSequence<u16> = FramerBuilder::new(plane, 64)
        .codec_version(1, TimeMode::DeltaT)
        .time_parameters(50000, 1000, 1000, 50.0)
        .mode(INSTANTANEOUS)
        .source(U8, FramedU8)
        .finish();

    for i in 0..5 {
        for j in 0..5 {
            let mut event: Event = Event {
                coord: Coord {
                    x: i,
                    y: j,
                    c: None,
                },
                d: 5,
                delta_t: 5100,
            };
            let filled = frame_sequence.ingest_event(&mut event);
            if i < 4 || j < 4 {
                assert!(!filled)
            } else {
                assert!(filled)
            }
        }
        if i < 4 {
            assert!(!frame_sequence.is_frame_filled(0).unwrap());
        } else {
            assert!(frame_sequence.is_frame_filled(0).unwrap());
        }
    }
    let n: u32 = rand::thread_rng().gen();
    let path = "./TEST_".to_owned() + n.to_string().as_str() + ".addr";
    let file = File::create(&path).unwrap();
    let mut output_writer = BufWriter::new(file);

    assert_eq!(fs::metadata(&path).unwrap().len(), 0);
    match frame_sequence.write_multi_frame_bytes(&mut output_writer) {
        Ok(6) => {
            output_writer.flush().unwrap();
            drop(output_writer);

            assert_eq!(fs::metadata(&path).unwrap().len(), 300);
            fs::remove_file(&path).unwrap(); // Don't check the error
        }
        _ => {
            panic!("fail")
        }
    }
}

#[test]
fn get_frame_bytes_u32() {
    use adder_codec_rs::framer::driver::FramerMode::INSTANTANEOUS;
    use adder_codec_rs::framer::driver::{FrameSequence, Framer};
    let plane = PlaneSize::new(5, 5, 1).unwrap();
    let mut frame_sequence: FrameSequence<u32> = FramerBuilder::new(plane, 46)
        .codec_version(1, TimeMode::DeltaT)
        .time_parameters(50000, 1000, 1000, 50.0)
        .mode(INSTANTANEOUS)
        .source(U8, FramedU8)
        .finish();

    for i in 0..5 {
        for j in 0..5 {
            let mut event: Event = Event {
                coord: Coord {
                    x: i,
                    y: j,
                    c: None,
                },
                d: 5,
                delta_t: 5100,
            };
            let filled = frame_sequence.ingest_event(&mut event);
            if i < 4 || j < 4 {
                assert!(!filled)
            } else {
                assert!(filled)
            }
        }
        if i < 4 {
            assert!(!frame_sequence.is_frame_filled(0).unwrap());
        } else {
            assert!(frame_sequence.is_frame_filled(0).unwrap());
        }
    }
    let n: u32 = rand::thread_rng().gen();
    let path = "./TEST_".to_owned() + n.to_string().as_str() + ".addr";
    let file = File::create(&path).unwrap();
    let mut output_writer = BufWriter::new(file);

    assert_eq!(fs::metadata(&path).unwrap().len(), 0);
    match frame_sequence.write_multi_frame_bytes(&mut output_writer) {
        Ok(6) => {
            output_writer.flush().unwrap();
            drop(output_writer);

            assert_eq!(fs::metadata(&path).unwrap().len(), 600);
            fs::remove_file(&path).unwrap(); // Don't check the error
        }
        _ => {
            panic!("fail")
        }
    }
}

#[test]
fn get_frame_bytes_u64() {
    use adder_codec_rs::framer::driver::FramerMode::INSTANTANEOUS;
    use adder_codec_rs::framer::driver::{FrameSequence, Framer};
    let plane = PlaneSize::new(5, 5, 1).unwrap();
    let mut frame_sequence: FrameSequence<u64> = FramerBuilder::new(plane, 64)
        .codec_version(1, TimeMode::DeltaT)
        .time_parameters(50000, 1000, 1000, 50.0)
        .mode(INSTANTANEOUS)
        .source(U8, FramedU8)
        .finish();

    for i in 0..5 {
        for j in 0..5 {
            let mut event: Event = Event {
                coord: Coord {
                    x: i,
                    y: j,
                    c: None,
                },
                d: 5,
                delta_t: 5100,
            };
            let filled = frame_sequence.ingest_event(&mut event);
            if i < 4 || j < 4 {
                assert!(!filled)
            } else {
                assert!(filled)
            }
        }
        if i < 4 {
            assert!(!frame_sequence.is_frame_filled(0).unwrap());
        } else {
            assert!(frame_sequence.is_frame_filled(0).unwrap());
        }
    }
    let n: u32 = rand::thread_rng().gen();
    let path = "./TEST_".to_owned() + n.to_string().as_str() + ".addr";
    let file = File::create(&path).unwrap();
    let mut output_writer = BufWriter::new(file);

    assert_eq!(fs::metadata(&path).unwrap().len(), 0);
    match frame_sequence.write_multi_frame_bytes(&mut output_writer) {
        Ok(6) => {
            output_writer.flush().unwrap();
            drop(output_writer);

            assert_eq!(fs::metadata(&path).unwrap().len(), 1200);
            fs::remove_file(&path).unwrap(); // Don't check the error
        }
        _ => {
            panic!("fail")
        }
    }
}

#[test]
fn test_get_empty_frame() {
    use adder_codec_rs::framer::driver::FramerMode::INSTANTANEOUS;
    use adder_codec_rs::framer::driver::{FrameSequence, Framer};
    let plane = PlaneSize::new(5, 5, 1).unwrap();
    let mut frame_sequence: FrameSequence<u8> = FramerBuilder::new(plane, 64)
        .codec_version(1, TimeMode::DeltaT)
        .time_parameters(50000, 1000, 1000, 50.0)
        .mode(INSTANTANEOUS)
        .source(U8, FramedU8)
        .finish();
    let n: u32 = rand::thread_rng().gen();
    let path = "./TEST_".to_owned() + n.to_string().as_str() + ".addr";
    let file = File::create(&path).unwrap();
    let mut output_writer = BufWriter::new(file);
    frame_sequence
        .write_frame_bytes(&mut output_writer)
        .unwrap();
    output_writer.flush().unwrap();
    assert_eq!(fs::metadata(&path).unwrap().len(), 25); // Even if it's all empty data, still want
                                                        // to perform the write. Up to the user to make sure that the frame is filled.
    let mut event: Event = Event {
        coord: Coord {
            x: 0,
            y: 0,
            c: None,
        },
        d: 5,
        delta_t: 500,
    };

    // TODO: check that events ingested with times after they've been popped off don't actually get
    // integrated!
    let filled = frame_sequence.ingest_event(&mut event);
    assert!(!filled);
    fs::remove_file(&path).unwrap();
}

#[test]
fn test_sample_unordered() {
    let input_path = "./tests/samples/sample_3_unordered.adder";
    let tmp = File::open(input_path).unwrap();
    let bufreader = BufReader::new(tmp);
    let compression = RawInput::new();

    let mut bitreader = BitReader::endian(bufreader, BigEndian);
    let mut reader = Decoder::new_raw(compression, &mut bitreader).unwrap();

    let output_path = Path::new("./tests/samples/temp_sample_3_unordered");
    let mut output_stream = BufWriter::new(File::create(output_path).unwrap());

    let reconstructed_frame_rate = 60.0;
    // For instantaneous reconstruction, make sure the frame rate matches the source video rate
    assert_eq!(
        reader.meta().tps / reader.meta().ref_interval,
        reconstructed_frame_rate as u32
    );

    let mut frame_sequence: FrameSequence<u8> = FramerBuilder::new(reader.meta().plane, 64)
        .codec_version(reader.meta().codec_version, TimeMode::DeltaT)
        .time_parameters(
            reader.meta().tps,
            reader.meta().ref_interval,
            reader.meta().delta_t_max,
            reconstructed_frame_rate,
        )
        .mode(INSTANTANEOUS)
        .source(reader.get_source_type(), reader.meta().source_camera)
        .finish();
    let mut frame_count = 0;
    loop {
        match reader.digest_event(&mut bitreader) {
            Ok(mut event) => {
                if frame_sequence.ingest_event(&mut event) {
                    match frame_sequence.write_multi_frame_bytes(&mut output_stream) {
                        Ok(0) => {
                            panic!("should have frame")
                        }
                        Ok(frames_returned) => {
                            frame_count += frames_returned;
                        }
                        Err(e) => {
                            panic!("error writing frame: {e}")
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

    assert_eq!(frame_count, 405);

    output_stream.flush().unwrap();
    let output = if !cfg!(target_os = "windows") {
        Command::new("sh")
            .arg("-c")
            .arg("cmp ./temp_sample_3_unordered ./sample_3.gray")
            .output()
            .expect("failed to execute process")
    } else {
        fs::remove_file(output_path).unwrap();
        return;
    };
    assert_eq!(output.stdout.len(), 0);
    fs::remove_file(output_path).unwrap();
}

#[test]
fn test_sample_ordered() {
    let input_path = "./tests/samples/sample_3_ordered.adder";
    let tmp = File::open(input_path).unwrap();
    let bufreader = BufReader::new(tmp);
    let compression = RawInput::new();

    let mut bitreader = BitReader::endian(bufreader, BigEndian);
    let mut reader = Decoder::new_raw(compression, &mut bitreader).unwrap();

    let output_path = Path::new("./tests/samples/temp_sample_3_ordered");
    let mut output_stream = BufWriter::new(File::create(output_path).unwrap());

    let reconstructed_frame_rate = 60.0;
    // For instantaneous reconstruction, make sure the frame rate matches the source video rate
    assert_eq!(
        reader.meta().tps / reader.meta().ref_interval,
        reconstructed_frame_rate as u32
    );

    let mut frame_sequence: FrameSequence<u8> = FramerBuilder::new(reader.meta().plane, 64)
        .codec_version(reader.meta().codec_version, TimeMode::DeltaT)
        .time_parameters(
            reader.meta().tps,
            reader.meta().ref_interval,
            reader.meta().delta_t_max,
            reconstructed_frame_rate,
        )
        .mode(INSTANTANEOUS)
        .source(reader.get_source_type(), reader.meta().source_camera)
        .finish();
    let mut frame_count = 0;
    loop {
        match reader.digest_event(&mut bitreader) {
            Ok(mut event) => {
                if frame_sequence.ingest_event(&mut event) {
                    match frame_sequence.write_multi_frame_bytes(&mut output_stream) {
                        Ok(0) => {
                            panic!("should have frame")
                        }
                        Ok(frames_returned) => {
                            frame_count += frames_returned;
                        }
                        Err(e) => {
                            panic!("error writing frame: {e}")
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

    assert_eq!(frame_count, 405);

    output_stream.flush().unwrap();
    let output = if !cfg!(target_os = "windows") {
        Command::new("sh")
            .arg("-c")
            .arg("cmp ./temp_sample_3_ordered ./sample_3.gray")
            .output()
            .expect("failed to execute process")
    } else {
        fs::remove_file(output_path).unwrap();
        return;
    };
    assert_eq!(output.stdout.len(), 0);
    fs::remove_file(output_path).unwrap();
}

#[test]
fn test_framed_to_adder_bunny4() {
    let data = fs::read_to_string("./tests/samples/bunny4.json").expect("Unable to read file");
    let gt_events: Vec<Event> = serde_json::from_str(data.as_str()).unwrap();
    let mut source: Framed<BufWriter<File>> =
        Framed::new("./tests/samples/bunny_crop4.mp4".to_string(), false, 1.0)
            .unwrap()
            // .chunk_rows(64)
            .frame_start(361)
            .unwrap()
            .contrast_thresholds(5, 5)
            .show_display(false)
            .quality_manual(5, 5, 1, 1, 0.0)
            .auto_time_parameters(5000, 240_000, Some(DeltaT))
            .unwrap();

    let frame_max = 250;

    let mut event_count: usize = 0;
    let mut test_events = Vec::new();
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(current_num_threads())
        .build()
        .unwrap();
    loop {
        match source.consume(1, &pool) {
            Ok(events_events) => {
                for events in events_events {
                    for event in events {
                        if event.coord.x == 0 && event.coord.y == 186 {
                            let gt_event = gt_events[event_count];
                            assert_eq!(event, gt_event);
                            test_events.push(event);
                            event_count += 1;
                        }
                    }
                }
            }
            Err(e) => {
                println!("Err: {e:?}");
                break;
            }
        }

        let video = source.get_video_ref();
        if frame_max != 0 && video.state.in_interval_count >= frame_max {
            break;
        }
    }
    assert_eq!(gt_events.len(), test_events.len());
    // let j = serde_json::to_string(&test_events).unwrap();
    // fs::write("./tmp.txt", j).expect("Unable to write file");
}

#[test]
fn array3_test() {
    let mut data = Vec::new();
    let height = 6_usize;
    let width = 6_usize;
    let channels = 3_usize;
    for _y in 0..height {
        for _x in 0..width {
            for _c in 0..channels {
                let px = 0;
                data.push(px);
            }
        }
    }

    let mut event_pixel_trees: Array3<i32> =
        Array3::from_shape_vec((height, width, channels), data).unwrap();
    let tmp = event_pixel_trees
        .axis_chunks_iter_mut(Axis(0), 4)
        .enumerate()
        .len();
    println!("{tmp}");

    let ret: Vec<i32> = event_pixel_trees
        .axis_chunks_iter_mut(Axis(0), 4)
        .enumerate()
        .map(|(chunk_idx, chunk)| {
            if chunk_idx == 0 {
                assert_eq!(chunk.len(), 72);
            }
            if chunk_idx == 1 {
                assert_eq!(chunk.len(), 36);
            }
            1
        })
        .collect();
    assert_eq!(ret, vec![1, 1]);
}
