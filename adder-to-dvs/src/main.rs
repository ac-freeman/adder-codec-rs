use adder_codec_rs::{Event, D_SHIFT, D_ZERO_INTEGRATION};
use std::cmp::max;
use std::collections::VecDeque;
use std::error::Error;
use std::fs::File;
use std::io::{BufReader, BufWriter, Write};
use std::path::Path;
use std::{error, io};

use adder_codec_rs::codec::Codec;
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
    /// Input ADΔER video path
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
/// This program transcodes an ADΔER file to DVS events in a human-readable text representation.
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

    let mut stream: Raw = Codec::new();
    let file = File::open(file_path)?;
    stream.set_input_stream(Some(BufReader::new(file)));
    let header_bytes = stream.decode_header().expect("Invalid header");

    let first_event_position = stream.get_input_stream_position()?;

    let eof_position_bytes = stream.get_eof_position()?;
    let _file_size = Path::new(file_path).metadata()?.len();
    let num_events = (eof_position_bytes - 1 - header_bytes as u64) / stream.event_size as u64;
    let divisor = num_events / 100;

    let stdout = io::stdout();
    let mut handle = io::BufWriter::new(stdout.lock());

    stream.set_input_stream_position(first_event_position)?;

    let mut video_writer: Option<BufWriter<File>> = match File::create(raw_path) {
        Ok(file) => Some(BufWriter::new(file)),
        Err(_) => None,
    };
    let mut text_writer: BufWriter<File> = BufWriter::new(File::create(output_text_path)?);
    {
        // Write the width and height as first line header
        let dims_str = stream.plane.w().to_string() + " " + &*stream.plane.h().to_string() + "\n";
        let amt = text_writer
            .write(dims_str.as_ref())
            .expect("Could not write");
        debug_assert_eq!(amt, dims_str.len());
    }

    let mut event_count: u64 = 0;

    let mut pixels: Array3<Option<DvsPixel>> = {
        let mut data: Vec<Option<DvsPixel>> = Vec::new();
        for _ in 0..stream.plane.h() {
            for _ in 0..stream.plane.w() {
                for _ in 0..stream.plane.c() {
                    let px = None;
                    data.push(px);
                }
            }
        }

        Array3::from_shape_vec(
            (
                stream.plane.h().into(),
                stream.plane.w().into(),
                stream.plane.c().into(),
            ),
            data,
        )?
    };

    let mut event_counts: Array3<u16> = Array3::zeros((
        stream.plane.h().into(),
        stream.plane.w().into(),
        stream.plane.c().into(),
    ));

    let mut instantaneous_frame_deque = {
        let mut instantaneous_frame = Mat::default();
        match stream.plane.c() {
            1 => unsafe {
                instantaneous_frame.create_rows_cols(
                    stream.plane.h() as i32,
                    stream.plane.w() as i32,
                    CV_8U,
                )?;
            },
            _ => unsafe {
                instantaneous_frame.create_rows_cols(
                    stream.plane.h() as i32,
                    stream.plane.w() as i32,
                    CV_8UC3,
                )?;
            },
        }

        VecDeque::from([instantaneous_frame])
    };

    match instantaneous_frame_deque
        .back_mut()
        .expect("Could not get back of deque")
        .data_bytes_mut()
    {
        Ok(bytes) => {
            for byte in bytes {
                *byte = 128;
            }
        }
        Err(e) => {
            return Err(Box::new(e));
        }
    };

    let frame_length = (stream.tps as f32 / args.fps) as u128; // length in ticks
    let mut frame_count = 0_usize;
    let mut current_t = 0;
    let mut max_px_event_count = 0;

    loop {
        if event_count % divisor == 0 {
            write!(
                handle,
                "\rTranscoding ADΔER to DVS...{}%",
                (event_count * 100) / num_events
            )?;
            handle.flush()?;
        }
        if current_t > (frame_count as u128 * frame_length) + stream.delta_t_max as u128 * 4 {
            match instantaneous_frame_deque.pop_front() {
                None => {}
                Some(frame) => {
                    if args.show_display {
                        show_display_force("DVS", &frame, 1)?;
                    }
                    match video_writer {
                        None => {}
                        Some(ref mut writer) => {
                            write_frame_to_video(&frame, writer)?;
                        }
                    }
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
                        d if d <= D_ZERO_INTEGRATION => {
                            pixels[[y, x, c]] = Some(DvsPixel {
                                d: event.d,
                                frame_intensity_ln: event_to_frame_intensity(&event, frame_length),
                                t: event.delta_t as u128,
                            });
                        }
                        _ => {
                            dbg!(event);
                            return Err("Shouldn't happen".into());
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
                                        )?;
                                        let dvs_string = px.t.to_string()
                                            + " "
                                            + x.to_string().as_str()
                                            + " "
                                            + y.to_string().as_str()
                                            + " "
                                            + "1\n";
                                        let amt = text_writer
                                            .write(dvs_string.as_ref())
                                            .expect("Could not write");
                                        debug_assert_eq!(amt, dvs_string.len());
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
                                        )?;
                                        let dvs_string = px.t.to_string()
                                            + " "
                                            + x.to_string().as_str()
                                            + " "
                                            + y.to_string().as_str()
                                            + " "
                                            + "-1\n";
                                        let amt = text_writer
                                            .write(dvs_string.as_ref())
                                            .expect("Could not write");
                                        debug_assert_eq!(amt, dvs_string.len());
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
        for y in 0..stream.plane.h() as i32 {
            for x in 0..stream.plane.w() as i32 {
                for c in 0..stream.plane.c() as i32 {
                    *event_count_mat.at_3d_unchecked_mut(y, x, c)? =
                        ((event_counts[[y as usize, x as usize, c as usize]] as f32
                            / max_px_event_count as f32)
                            * 255.0) as u8;
                }
            }
        }
    }

    for frame in instantaneous_frame_deque {
        if args.show_display {
            show_display_force("DVS", &frame, 1)?;
        }
        match video_writer {
            None => {}
            Some(ref mut writer) => {
                write_frame_to_video(&frame, writer)?;
            }
        }
    }
    println!("\n");
    if args.show_display {
        show_display_force("Event counts", &event_count_mat, 0)?;
    }
    encode_video_ffmpeg(raw_path, output_video_path)?;

    handle.flush()?;
    println!("Finished!");
    Ok(())
}

fn set_instant_dvs_pixel(
    event: Event,
    frames: &mut VecDeque<Mat>,
    frame_idx: usize,
    frame_count: usize,
    value: u128,
) -> Result<(), Box<dyn Error>> {
    // Grow the deque if necessary
    let grow_len = frame_idx as i32 - frame_count as i32 - frames.len() as i32 + 1;

    for _ in 0..grow_len {
        frames.push_back(frames[0].clone());
        // Clear the instantaneous frame
        match frames
            .back_mut()
            .expect("Could not get back of deque")
            .data_bytes_mut()
        {
            Ok(bytes) => {
                for byte in bytes {
                    *byte = 128;
                }
            }
            Err(e) => {
                return Err(e.into());
            }
        };
    }

    unsafe {
        let px: &mut u8 = match event.coord.c {
            None => frames[frame_idx - frame_count]
                .at_2d_unchecked_mut(event.coord.y.into(), event.coord.x.into())?,
            Some(c) => frames[frame_idx - frame_count].at_3d_unchecked_mut(
                event.coord.y.into(),
                event.coord.x.into(),
                c.into(),
            )?,
        };
        *px = value as u8;
        // match value {
        //     128 => *px = 128,
        //     a => *px = (*px as i16 + a) as u8,
        // }
    }
    Ok(())
}

fn event_to_frame_intensity(event: &Event, frame_length: u128) -> f64 {
    if event.d == D_ZERO_INTEGRATION {
        return 0.0;
    }
    match event.delta_t {
        0 => ((D_SHIFT[event.d as usize] as f64 * frame_length as f64) / 255.0).ln_1p(),
        _ => (((D_SHIFT[event.d as usize] as f64 / event.delta_t as f64) * frame_length as f64)
            / 255.0)
            .ln_1p(),
    }
}
