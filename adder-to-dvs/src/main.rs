use adder_codec_core::codec::CodecMetadata;
use adder_codec_core::*;
use clap::Parser;
use ndarray::Array3;
use std::cmp::max;
use std::collections::VecDeque;
use std::error::Error;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::option::Option;
use std::path::PathBuf;
use std::{error, io};
use video_rs::{Encoder, EncoderSettings, Options, PixelFormat};

/// Command line argument parser
#[derive(Parser, Debug, Default, Clone)]
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

    /// Size of the frame buffer in seconds
    #[clap(long, default_value_t = 10.0)]
    pub buffer_secs: f32,

    /// DVS contrast threshold for inferring events
    #[clap(long, default_value_t = 0.01)]
    pub theta: f64,

    #[clap(short, long, action)]
    pub show_display: bool,

    /// For the framed video, scale the playback speed by this factor (<1 is slower, >1 is faster)
    #[clap(long, default_value_t = 1.0)]
    pub playback_slowdown: f64,
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
    dbg!(args.clone());
    let file_path = args.input.as_str();

    let output_text_path = args.output_text.as_str();
    let output_video_path = args.output_video.as_str();

    let (mut stream, mut bitreader) = open_file_decoder(file_path)?;

    let first_event_position = stream.get_input_stream_position(&mut bitreader)?;

    let eof_position_bytes = stream.get_eof_position(&mut bitreader)?;

    let meta = *stream.meta();

    // TODO: Need a different mechanism for compressed files
    let num_events = (eof_position_bytes - 1 - meta.header_size as u64) / meta.event_size as u64;
    let divisor = num_events / 100;

    let stdout = io::stdout();
    let mut handle = io::BufWriter::new(stdout.lock());

    stream.set_input_stream_position(&mut bitreader, first_event_position)?;

    let mut video_writer: Option<Encoder> = match File::create(output_video_path) {
        Ok(_) => {
            let mut options = std::collections::HashMap::new();
            options.insert("crf".to_string(), "0".to_string());
            options.insert("preset".to_string(), "veryslow".to_string());
            options.insert("qp".to_string(), "0".to_string());
            let opts: Options = options.into();

            let settings = EncoderSettings::for_h264_custom(
                meta.plane.w_usize(),
                meta.plane.h_usize(),
                PixelFormat::YUV420P,
                opts,
            );
            let encoder = Encoder::new(&PathBuf::from(output_video_path).into(), settings)?;
            Some(encoder)
        }
        Err(_) => None,
    };
    let mut text_writer: BufWriter<File> = BufWriter::new(File::create(output_text_path)?);
    {
        // Write the width and height as first line header
        let dims_str = meta.plane.w().to_string() + " " + &*meta.plane.h().to_string() + "\n";
        let amt = text_writer
            .write(dims_str.as_ref())
            .expect("Could not write");
        debug_assert_eq!(amt, dims_str.len());
    }

    let mut event_count: u64 = 0;

    let mut pixels: Array3<Option<DvsPixel>> = {
        let mut data: Vec<Option<DvsPixel>> = Vec::new();
        for _ in 0..meta.plane.h() {
            for _ in 0..meta.plane.w() {
                for _ in 0..meta.plane.c() {
                    let px = None;
                    data.push(px);
                }
            }
        }

        Array3::from_shape_vec(
            (
                meta.plane.h().into(),
                meta.plane.w().into(),
                meta.plane.c().into(),
            ),
            data,
        )?
    };

    let mut event_counts: Array3<u16> = Array3::zeros((
        meta.plane.h().into(),
        meta.plane.w().into(),
        meta.plane.c().into(),
    ));

    let mut instantaneous_frame_deque = VecDeque::from([create_blank_dvs_frame(&meta)?]);

    let frame_length = (meta.tps as f32 / args.fps) as u128; // length in ticks
    let frame_duration = 1.0 / args.fps as f64; // length in seconds

    let mut current_frame_time = 0.0;
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
        if current_t
            > (frame_count as u128 * frame_length) + (meta.tps as f32 * args.buffer_secs) as u128
        {
            match instantaneous_frame_deque.pop_front() {
                None => {}
                Some(frame) => {
                    // if args.show_display {
                    //     show_display_force("DVS", &frame, 1)?;
                    // }
                    match video_writer {
                        None => {}
                        Some(ref mut encoder) => {
                            write_frame_to_video(
                                &frame,
                                encoder,
                                video_rs::Time::from_secs_f64(
                                    current_frame_time / args.playback_slowdown,
                                ),
                            )?;
                            current_frame_time += frame_duration;
                        }
                    }
                }
            }
            frame_count += 1;
        }

        match stream.digest_event(&mut bitreader) {
            Ok(mut event) => {
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
                                frame_intensity_ln: event_to_frame_intensity(
                                    &event,
                                    meta.ref_interval as u128,
                                ),
                                t: event.t as u128,
                            });
                        }
                        _ => {
                            dbg!(event);
                            return Err("Shouldn't happen".into());
                        }
                    },
                    Some(px) => {
                        let old_t = px.t;
                        if meta.time_mode == TimeMode::DeltaT {
                            px.t += event.t as u128;
                        } else {
                            px.t = event.t as u128;
                            event.t = event.t.saturating_sub(old_t as u32);
                        }

                        if is_framed(meta.source_camera) {
                            px.t = if px.t % meta.ref_interval as u128 == 0 {
                                px.t
                            } else {
                                (((px.t / meta.ref_interval as u128) + 1)
                                    * meta.ref_interval as u128)
                            };
                        }

                        current_t = max(px.t, current_t);

                        // Base the frame idx on the START of the ADDER event, so we just have the
                        // instantaneous moment that the intensity change happened
                        let frame_idx = ((old_t + 1) / frame_length) as usize;

                        match event.d {
                            255 => {
                                // ignore empty events
                                continue; // Don't update d with this
                            }
                            _ => {
                                let x = event.coord.x;
                                let y = event.coord.y;
                                let new_intensity_ln =
                                    event_to_frame_intensity(&event, meta.ref_interval as u128);
                                match (new_intensity_ln, px.frame_intensity_ln) {
                                    (a, b) if a >= b + args.theta => {
                                        // Fire a positive polarity event
                                        set_instant_dvs_pixel(
                                            event,
                                            &meta,
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
                                    (a, b) if a <= b - args.theta => {
                                        // Fire a negative polarity event
                                        set_instant_dvs_pixel(
                                            event,
                                            &meta,
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

    if instantaneous_frame_deque.is_empty() {
        instantaneous_frame_deque.push_back(create_blank_dvs_frame(&meta)?);
    }

    let mut event_count_mat = instantaneous_frame_deque[0].clone();
    unsafe {
        for y in 0..meta.plane.h_usize() {
            for x in 0..meta.plane.w_usize() {
                for c in 0..meta.plane.c_usize() {
                    event_count_mat[[y, x, c]] = ((event_counts[[y, x, c]] as f32
                        / max_px_event_count as f32)
                        * 255.0) as u8;
                }
            }
        }
    }

    for frame in instantaneous_frame_deque {
        // if args.show_display {
        //     show_display_force("DVS", &frame, 1)?;
        // }
        match video_writer {
            None => {}
            Some(ref mut encoder) => {
                write_frame_to_video(
                    &frame,
                    encoder,
                    video_rs::Time::from_secs_f64(current_frame_time / args.playback_slowdown),
                )?;
                current_frame_time += frame_duration;
            }
        }
    }
    println!("\n");

    // TODO: restore this functionality
    // if args.show_display {
    //     show_display_force("Event counts", &event_count_mat, 0)?;
    // }

    handle.flush()?;
    println!("Finished!");
    Ok(())
}

fn set_instant_dvs_pixel(
    event: Event,
    meta: &CodecMetadata,
    frames: &mut VecDeque<Array3<u8>>,
    frame_idx: usize,
    frame_count: usize,
    value: u128,
) -> Result<(), Box<dyn Error>> {
    // Grow the deque if necessary
    let grow_len = frame_idx as i32 - frame_count as i32 - frames.len() as i32 + 1;

    for _ in 0..grow_len {
        frames.push_back(create_blank_dvs_frame(&meta)?);
    }

    unsafe {
        if frame_idx >= frame_count {
            frames[frame_idx - frame_count][[event.coord.y.into(), event.coord.x.into(), 0]] =
                value as u8;
            frames[frame_idx - frame_count][[event.coord.y.into(), event.coord.x.into(), 1]] =
                value as u8;
            frames[frame_idx - frame_count][[event.coord.y.into(), event.coord.x.into(), 2]] =
                value as u8;
        }
    }
    Ok(())
}

fn event_to_frame_intensity(event: &Event, frame_length: u128) -> f64 {
    if event.d == D_ZERO_INTEGRATION {
        return 0.0;
    }
    let tmp = (D_SHIFT[event.d as usize] as f64 * frame_length as f64);
    match event.t {
        0 => ((D_SHIFT[event.d as usize] as f64 * frame_length as f64) / 255.0).ln_1p(),
        _ => (((D_SHIFT[event.d as usize] as f64 / event.t as f64) * frame_length as f64) / 255.0)
            .ln_1p(),
    }
}

fn create_blank_dvs_frame(meta: &CodecMetadata) -> Result<Array3<u8>, Box<dyn Error>> {
    let instantaneous_frame: Array3<u8> = Array3::from_shape_vec(
        (meta.plane.h_usize(), meta.plane.w_usize(), 3),
        vec![128_u8; meta.plane.h_usize() * meta.plane.w_usize() * 3],
    )?;
    Ok(instantaneous_frame)
}

pub fn write_frame_to_video(
    frame: &video_rs::Frame,
    encoder: &mut video_rs::Encoder,
    timestamp: video_rs::Time,
) -> Result<(), Box<dyn Error>> {
    encoder.encode(&frame, &timestamp).map_err(|e| e.into())
}
