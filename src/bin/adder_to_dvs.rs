use adder_codec_rs::raw::raw_stream::RawStream;
use adder_codec_rs::{Codec, Event, D_SHIFT};
use std::cmp::max;
use std::collections::VecDeque;
use std::fs::File;
use std::io::{BufWriter, SeekFrom, Write};
use std::path::Path;
use std::{error, io};

use adder_codec_rs::transcoder::source::video::{show_display, show_display_force};
use clap::Parser;
use ndarray::{Array3, Shape};
use opencv::core::{Mat, MatTrait, MatTraitConstManual, MatTraitManual, CV_8U, CV_8UC3};
use std::option::Option;
use tokio::io::AsyncSeekExt;

/// Command line argument parser
#[derive(Parser, Debug, Default)]
#[clap(author, version, about, long_about = None)]
pub struct MyArgs {
    /// Input ADDER video path
    #[clap(short, long)]
    pub(crate) input: String,

    /// Output DVS event text file path
    #[clap(short, long)]
    pub(crate) output: String,
}

struct DvsPixel {
    d: u8,
    frame_intensity_ln: f64,
    t: u128,
}

fn main() -> Result<(), Box<dyn error::Error>> {
    let args: MyArgs = MyArgs::parse();
    let file_path = args.input.as_str();

    let output_file_path = args.output.as_str();

    let mut stream: RawStream = Codec::new();
    stream.open_reader(file_path).expect("Invalid path");
    let header_bytes = stream.decode_header().expect("Invalid header");

    let first_event_position = stream.get_input_stream_position().unwrap();

    let eof_position_bytes = stream.get_eof_position().unwrap();
    let file_size = Path::new(file_path).metadata().unwrap().len();
    let num_events = (eof_position_bytes - 1 - header_bytes as u64) / stream.event_size as u64;
    let divisor = num_events as u64 / 100;

    let stdout = io::stdout();
    let mut handle = io::BufWriter::new(stdout.lock());

    stream.set_input_stream_position(first_event_position)?;

    let mut video_writer: BufWriter<File> = BufWriter::new(File::create("./dvs.gray8").unwrap());

    let mut event_count: u64 = 0;

    let mut data: Vec<Option<DvsPixel>> = Vec::new();
    for _ in 0..stream.height {
        for _ in 0..stream.width {
            for _ in 0..stream.channels {
                let px = None;
                data.push(px);
            }
        }
    }

    let mut pixels: Array3<Option<DvsPixel>> = Array3::from_shape_vec(
        (
            stream.height.into(),
            stream.width.into(),
            stream.channels.into(),
        ),
        data,
    )
    .unwrap();

    let mut event_counts: Array3<u16> = Array3::zeros((
        stream.height.into(),
        stream.width.into(),
        stream.channels.into(),
    ));

    let mut instantaneous_frame = Mat::default();
    match stream.channels {
        1 => unsafe {
            instantaneous_frame
                .create_rows_cols(stream.height as i32, stream.width as i32, CV_8U)
                .unwrap();
        },
        _ => unsafe {
            instantaneous_frame
                .create_rows_cols(stream.height as i32, stream.width as i32, CV_8UC3)
                .unwrap();
        },
    }

    let mut instantaneous_frame_deque = VecDeque::from([instantaneous_frame]);

    let frame_length = (stream.tps / 100) as u128; // length in ticks
    let mut frame_count = 0_usize;
    let mut current_t = 0;
    let mut max_px_event_count = 0;

    loop {
        if event_count % divisor == 0 {
            write!(
                handle,
                "\rTranscoding ADDER to DVS...{}%",
                (event_count * 100) / num_events as u64
            )?;
            handle.flush().unwrap();
        }
        if current_t > (frame_count as u128 * frame_length) + stream.delta_t_max as u128 {
            match instantaneous_frame_deque.pop_front() {
                None => {}
                Some(frame) => {
                    show_display_force("DVS", &frame, 1);
                    write_frame_to_video(&frame, &mut video_writer);
                }
            }
            frame_count += 1;
        }

        match stream.decode_event() {
            Ok(event) => {
                if event.coord.y > 130 {
                    // dbg!(event);
                }
                event_count += 1;
                let y = event.coord.y as usize;
                let x = event.coord.x as usize;
                let c = event.coord.c.unwrap_or(0) as usize;
                event_counts[[y, x, c]] += 1;
                max_px_event_count = max(max_px_event_count, event_counts[[y, x, c]]);

                match &mut pixels[[y, x, c]] {
                    None => {
                        if event.d < 253 {
                            pixels[[y, x, c]] = Some(DvsPixel {
                                d: event.d,
                                frame_intensity_ln: event_to_frame_intensity(&event, frame_length),
                                t: event.delta_t as u128,
                            });
                        } else {
                            panic!("Shouldn't happen")
                        }
                    }
                    Some(px) => {
                        px.t += event.delta_t as u128;
                        current_t = max(px.t, current_t);
                        let frame_idx = (px.t / frame_length) as usize;

                        match event.d {
                            255 | 254 => {
                                // ignore empty events
                                continue; // Don't update d with this
                            }
                            _ => {
                                let new_intensity_ln =
                                    event_to_frame_intensity(&event, frame_length);
                                let c = 0.15;
                                match (new_intensity_ln, px.frame_intensity_ln) {
                                    (a, b) if a >= b + c => {
                                        // Fire a positive polarity event
                                        set_instant_dvs_pixel(
                                            event,
                                            &mut instantaneous_frame_deque,
                                            frame_idx,
                                            frame_count,
                                            255,
                                        );
                                    }
                                    (a, b) if a <= b - c => {
                                        // Fire a negative polarity event
                                        set_instant_dvs_pixel(
                                            event,
                                            &mut instantaneous_frame_deque,
                                            frame_idx,
                                            frame_count,
                                            0,
                                        );
                                    }
                                    (_, _) => {}
                                }
                                px.frame_intensity_ln = new_intensity_ln;
                            }
                        }
                        px.d = event.d;
                    }
                }
            }
            Err(_e) => {
                break;
            }
        }
    }

    let mut event_count_mat = instantaneous_frame_deque[0].clone();
    unsafe {
        for y in 0..stream.height as i32 {
            for x in 0..stream.width as i32 {
                for c in 0..stream.channels as i32 {
                    *event_count_mat.at_3d_unchecked_mut(y, x, c).unwrap() =
                        ((event_counts[[y as usize, x as usize, c as usize]] as f32
                            / max_px_event_count as f32)
                            * 255.0) as u8;
                }
            }
        }
    }

    for frame in instantaneous_frame_deque {
        show_display_force("DVS", &frame, 1);
        write_frame_to_video(&frame, &mut video_writer);
    }
    show_display_force("Event counts", &event_count_mat, 0);

    handle.flush().unwrap();
    println!("\nFinished!");
    Ok(())
}

fn set_instant_dvs_pixel(
    event: Event,
    frames: &mut VecDeque<Mat>,
    frame_idx: usize,
    frame_count: usize,
    value: u128,
) {
    // Grow the deque if necessary
    let grow_len = frame_idx as i32 - frame_count as i32 - frames.len() as i32 + 1;
    for _ in 0..grow_len {
        frames.push_back(frames[0].clone());
        // Clear the instantaneous frame
        match frames.back_mut().unwrap().data_bytes_mut() {
            Ok(bytes) => {
                for byte in bytes {
                    *byte = 128;
                }
            }
            Err(_) => {
                panic!("Mat error")
            }
        }
    }

    unsafe {
        let px: &mut u8 = match event.coord.c {
            None => frames[frame_idx - frame_count]
                .at_2d_mut(event.coord.y.into(), event.coord.x.into())
                .unwrap(),
            Some(c) => frames[frame_idx - frame_count]
                .at_3d_mut(event.coord.y.into(), event.coord.x.into(), c.into())
                .unwrap(),
        };
        *px = value as u8;
        // match value {
        //     128 => *px = 128,
        //     a => *px = (*px as i16 + a) as u8,
        // }
    }
}

fn event_to_frame_intensity(event: &Event, frame_length: u128) -> f64 {
    (((D_SHIFT[event.d as usize] as f64 / event.delta_t as f64) * frame_length as f64) / 255.0)
        .ln_1p()
}

fn write_frame_to_video(frame: &Mat, video_writer: &mut BufWriter<File>) {
    unsafe {
        for idx in 0..frame.size().unwrap().width * frame.size().unwrap().height {
            let val: *const u8 = frame.at_unchecked(idx).unwrap() as *const u8;
            video_writer
                .write(std::slice::from_raw_parts(val, 1))
                .unwrap();
        }
    }
}