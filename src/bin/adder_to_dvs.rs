use adder_codec_rs::raw::raw_stream::RawStream;
use adder_codec_rs::{Codec, Event, D, D_SHIFT};
use std::cmp::max;
use std::collections::VecDeque;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;
use std::{error, io};

use adder_codec_rs::transcoder::source::video::show_display_force;
use adder_codec_rs::utils::viz::{encode_video_ffmpeg, write_frame_to_video};
use clap::Parser;
use ndarray::Array3;
use opencv::core::{Mat, MatTrait, MatTraitManual, CV_8U, CV_8UC3};
use std::option::Option;

/// Command line argument parser
#[derive(Parser, Debug, Default)]
#[clap(author, version, about, long_about = None)]
pub struct MyArgs {
    /// Input ADDER video path
    #[clap(short, long)]
    pub(crate) input: String,

    /// Output DVS event text file path
    #[clap(long)]
    pub(crate) output_text: String,

    /// Output DVS event video file path
    #[clap(long)]
    pub(crate) output_video: String,

    #[clap(long, default_value_t = 100.0)]
    pub fps: f32,

    #[clap(short, long, action)]
    pub show_display: bool,
}

struct DvsPixel {
    d: u8,
    frame_intensity_ln: f64,
    t: u128,
}

///
/// This program transcodes an ADDER file to DVS events in a human-readable text representation.
/// Performance is fast. The resulting DVS stream is visualized during the transcode and written
/// out as an mp4 file.
///
#[allow(dead_code)]
fn main() -> Result<(), Box<dyn error::Error>> {
    let args: MyArgs = MyArgs::parse();
    let file_path = args.input.as_str();

    let output_text_path = args.output_text.as_str();
    let output_video_path = args.output_video.as_str();
    let raw_path = "./dvs.gray8";

    let mut stream: RawStream = Codec::new();
    stream.open_reader(file_path).expect("Invalid path");
    let header_bytes = stream.decode_header().expect("Invalid header");

    let first_event_position = stream.get_input_stream_position().unwrap();

    let eof_position_bytes = stream.get_eof_position().unwrap();
    let _file_size = Path::new(file_path).metadata().unwrap().len();
    let num_events = (eof_position_bytes - 1 - header_bytes as u64) / stream.event_size as u64;
    let divisor = num_events as u64 / 100;

    let stdout = io::stdout();
    let mut handle = io::BufWriter::new(stdout.lock());

    stream.set_input_stream_position(first_event_position)?;

    let mut video_writer: BufWriter<File> = BufWriter::new(File::create(raw_path).unwrap());
    let mut text_writer: BufWriter<File> = BufWriter::new(File::create(output_text_path).unwrap());
    {
        // Write the width and height as first line header
        let dims_str = stream.width.to_string() + " " + &*stream.height.to_string() + "\n";
        text_writer
            .write(dims_str.as_ref())
            .expect("Could not write");
    }

    let mut event_count: u64 = 0;

    let mut pixels: Array3<Option<DvsPixel>> = {
        let mut data: Vec<Option<DvsPixel>> = Vec::new();
        for _ in 0..stream.height {
            for _ in 0..stream.width {
                for _ in 0..stream.channels {
                    let px = None;
                    data.push(px);
                }
            }
        }

        Array3::from_shape_vec(
            (
                stream.height.into(),
                stream.width.into(),
                stream.channels.into(),
            ),
            data,
        )
        .unwrap()
    };

    let mut event_counts: Array3<u16> = Array3::zeros((
        stream.height.into(),
        stream.width.into(),
        stream.channels.into(),
    ));

    let mut instantaneous_frame_deque = {
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

        VecDeque::from([instantaneous_frame])
    };

    match instantaneous_frame_deque
        .back_mut()
        .unwrap()
        .data_bytes_mut()
    {
        Ok(bytes) => {
            for byte in bytes {
                *byte = 128;
            }
        }
        Err(_) => {
            panic!("Mat error")
        }
    }

    let frame_length = (stream.tps as f32 / args.fps) as u128; // length in ticks
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
                    if args.show_display {
                        show_display_force("DVS", &frame, 1);
                    }
                    write_frame_to_video(&frame, &mut video_writer);
                }
            }
            frame_count += 1;
        }

        match stream.decode_event() {
            Ok(event) => {
                event_count += 1;
                let y = event.coord.y as usize;
                let x = event.coord.x as usize;
                let c = event.coord.c.unwrap_or(0) as usize;
                event_counts[[y, x, c]] += 1;
                max_px_event_count = max(max_px_event_count, event_counts[[y, x, c]]);

                match &mut pixels[[y, x, c]] {
                    None => match event.d {
                        d if d <= 254 => {
                            pixels[[y, x, c]] = Some(DvsPixel {
                                d: event.d,
                                frame_intensity_ln: event_to_frame_intensity(&event, frame_length),
                                t: event.delta_t as u128,
                            });
                        }
                        _ => {
                            dbg!(event);
                            panic!("Shouldn't happen")
                        }
                    },
                    Some(px) => {
                        px.t += event.delta_t as u128;
                        current_t = max(px.t, current_t);
                        let frame_idx = (px.t / frame_length) as usize;

                        match event.d {
                            255 => {
                                // ignore empty events
                                continue; // Don't update d with this
                            }
                            _ => {
                                let x = event.coord.x;
                                let y = event.coord.y;
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
                                        let dvs_string = px.t.to_string()
                                            + " "
                                            + x.to_string().as_str()
                                            + " "
                                            + y.to_string().as_str()
                                            + " "
                                            + "1\n";
                                        text_writer
                                            .write(dvs_string.as_ref())
                                            .expect("Could not write");
                                        px.frame_intensity_ln = new_intensity_ln;
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
                                        let dvs_string = px.t.to_string()
                                            + " "
                                            + x.to_string().as_str()
                                            + " "
                                            + y.to_string().as_str()
                                            + " "
                                            + "-1\n";
                                        text_writer
                                            .write(dvs_string.as_ref())
                                            .expect("Could not write");
                                        px.frame_intensity_ln = new_intensity_ln;
                                    }
                                    (_, _) => {}
                                }
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

    text_writer.flush().expect("Could not flush");
    drop(text_writer);

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
        if args.show_display {
            show_display_force("DVS", &frame, 1);
        }
        write_frame_to_video(&frame, &mut video_writer);
    }
    println!("\n");
    if args.show_display {
        show_display_force("Event counts", &event_count_mat, 0);
    }
    encode_video_ffmpeg(raw_path, output_video_path);

    handle.flush().unwrap();
    println!("Finished!");
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
                .at_2d_unchecked_mut(event.coord.y.into(), event.coord.x.into())
                .unwrap(),
            Some(c) => frames[frame_idx - frame_count]
                .at_3d_unchecked_mut(event.coord.y.into(), event.coord.x.into(), c.into())
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
    if event.d == 254 {
        return 0.0;
    }
    match event.delta_t {
        0 => ((D_SHIFT[event.d as usize] as f64 * frame_length as f64) / 255.0).ln_1p(),
        _ => (((D_SHIFT[event.d as usize] as f64 / event.delta_t as f64) * frame_length as f64)
            / 255.0)
            .ln_1p(),
    }
}
