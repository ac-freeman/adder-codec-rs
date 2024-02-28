use adder_codec_core::codec::CodecMetadata;
use adder_codec_core::*;
use chrono::{DateTime, Local};
use clap::{Parser, ValueEnum};
use ndarray::Array3;
use serde::{Deserialize, Serialize};
use std::cmp::max;
use std::collections::VecDeque;
use std::error::Error;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::option::Option;
use std::path::PathBuf;
use std::time::Instant;
use std::{error, io};
use video_rs::{Encoder, EncoderSettings, Options, PixelFormat};

/// Command line argument parser
#[derive(Parser, Debug, Default, Clone)]
#[clap(author, version, about, long_about = None)]
pub struct MyArgs {
    /// Input ADΔER video path
    #[clap(short, long)]
    pub(crate) input: String,

    /// Output DVS event file path
    #[clap(long)]
    pub(crate) output_events: String,

    /// Format for output DVS events ('Binary' or 'Text')
    #[clap(long, value_enum, default_value_t = WriteMode::Binary)]
    pub(crate) output_mode: WriteMode,

    /// Output DVS event video file path
    #[clap(long, default_value = "")]
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

    #[clap(short, long, action)]
    pub reorder: bool,
}

struct DvsPixel {
    d: u8,
    frame_intensity_ln: f64,
    t: u128,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct DvsEvent {
    t: u32,
    x: u16,
    y: u16,
    p: u8,
}

impl PartialOrd for DvsEvent {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.t.cmp(&other.t))
    }
}

#[derive(Clone, ValueEnum, Debug, Default, Copy, PartialEq)]
enum WriteMode {
    Text,

    #[default]
    Binary,
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

    let output_events_path = args.output_events.as_str();
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

    let mut ordered_event_queue: Option<VecDeque<DvsEvent>> = if args.reorder {
        Some(VecDeque::new())
    } else {
        None
    };

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
    let mut output_events_writer: BufWriter<File> =
        BufWriter::new(File::create(output_events_path)?);
    {
        // Write the width and height as first lines of header
        // let dims_str = "Width " + meta.plane.w().to_string() + "\n";
        // let amt = output_events_writer
        //     .write(dims_str.as_ref())
        //     .expect("Could not write");

        write!(output_events_writer, "% Height {}\n", meta.plane.h())?;
        write!(output_events_writer, "% Width {}\n", meta.plane.w())?;
        write!(output_events_writer, "% Version 2\n")?;
        // Write the date and time
        let now = Local::now();
        let date_time_str = now.format("%Y-%m-%d %H:%M:%S").to_string();
        write!(output_events_writer, "% Date {}\n", date_time_str)?;
        write!(output_events_writer, "% end\n")?;

        if args.output_mode == WriteMode::Binary {
            let event_type_size: [u8; 2] = [0, 8];
            output_events_writer.write(&event_type_size)?;
        }
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
        // if event_count % divisor == 0 {
        //     write!(
        //         handle,
        //         "\rTranscoding ADΔER to DVS...{}%",
        //         (event_count * 100) / num_events
        //     )?;
        //     handle.flush()?;
        // }
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
                // if event.coord.x == 100 && event.coord.y == 100 {
                //     dbg!((event.t, event.d));
                //     if event.t == 20433028 {
                //         dbg!(event);
                //     }
                // }

                event_count += 1;
                let y = event.coord.y as usize;
                let x = event.coord.x as usize;
                let c = event.coord.c.unwrap_or(0) as usize;
                // event_counts[[y, x, c]] += 1;
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
                        // if event.coord.x == 100 && event.coord.y == 100 {
                        //     dbg!(px.frame_intensity_ln);
                        // }

                        let old_t = px.t;
                        let old_d = px.d;
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
                                ((px.t / meta.ref_interval as u128) + 1) * meta.ref_interval as u128
                            };
                        }

                        current_t = max(px.t, current_t);

                        // Base the frame idx on the START of the ADDER event, so we just have the
                        // instantaneous moment that the intensity change happened
                        let frame_idx = ((old_t + 1) / frame_length) as usize;

                        match event.d {
                            255 => {
                                // ignore empty events
                                px.d = event.d;
                                continue; // Don't update d with this
                            }
                            _ => {
                                // TODO: also do it if 2^D / delta_t is an integer!
                                let x = event.coord.x;
                                let y = event.coord.y;
                                let new_intensity_ln =
                                    event_to_frame_intensity(&event, meta.ref_interval as u128);

                                // if event.coord.x == 100 && event.coord.y == 100 {
                                //     dbg!(new_intensity_ln);
                                // }

                                // if old_d == 255
                                // // || ((D_SHIFT[event.d as usize] as f64
                                // //     * meta.ref_interval as f64)
                                // //     / event.t as f64)
                                // //     .fract()
                                // //     == 0.0
                                // {
                                if new_intensity_ln > 0.406
                                    && new_intensity_ln < 0.407
                                    && ((px.frame_intensity_ln > 1.0_f64.ln_1p() - args.theta)
                                        || (px.t == old_t && px.frame_intensity_ln > 0.6))
                                {
                                    if event.coord.x == 100 && event.coord.y == 100 {
                                        dbg!("A");
                                    }
                                    fire_dvs_event(
                                        true,
                                        x,
                                        y,
                                        old_t + 1,
                                        &mut output_events_writer,
                                        &mut ordered_event_queue,
                                        args.output_mode,
                                    )?;
                                    px.frame_intensity_ln = new_intensity_ln;
                                } else if new_intensity_ln > 0.406
                                    && new_intensity_ln < 0.407
                                    && ((px.frame_intensity_ln < 0.0_f64.ln_1p() + args.theta)
                                        || (px.t == old_t && px.frame_intensity_ln < 0.3))
                                {
                                    if event.coord.x == 100 && event.coord.y == 100 {
                                        dbg!("B");
                                    }
                                    fire_dvs_event(
                                        false,
                                        x,
                                        y,
                                        old_t + 1,
                                        &mut output_events_writer,
                                        &mut ordered_event_queue,
                                        args.output_mode,
                                    )?;
                                    px.frame_intensity_ln = new_intensity_ln;
                                } else if new_intensity_ln
                                    > px.frame_intensity_ln + args.theta / 2.0
                                {
                                    let mut mult = ((new_intensity_ln - px.frame_intensity_ln)
                                        / args.theta)
                                        as u32;
                                    // if mult == 0 {
                                    mult = 1;
                                    // }

                                    for i in 0..mult as u32 {
                                        fire_dvs_event(
                                            true,
                                            x,
                                            y,
                                            old_t + 1,
                                            &mut output_events_writer,
                                            &mut ordered_event_queue,
                                            args.output_mode,
                                        )?;
                                    }
                                    px.frame_intensity_ln = new_intensity_ln;
                                } else if new_intensity_ln
                                    < px.frame_intensity_ln - args.theta / 2.0
                                {
                                    let mut mult = ((px.frame_intensity_ln - new_intensity_ln)
                                        / args.theta)
                                        as u32;
                                    // if mult == 0 {
                                    mult = 1;
                                    // }
                                    for i in 0..mult {
                                        fire_dvs_event(
                                            false,
                                            x,
                                            y,
                                            old_t + 1,
                                            &mut output_events_writer,
                                            &mut ordered_event_queue,
                                            args.output_mode,
                                        )?;
                                    }
                                    px.frame_intensity_ln = new_intensity_ln;
                                }
                            } // }
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

    if let Some(queue) = &mut ordered_event_queue {
        while let Some(event) = queue.pop_front() {
            write_event_binary(&event, &mut output_events_writer)?;
        }
    }

    output_events_writer.flush().expect("Could not flush");
    drop(output_events_writer);

    if instantaneous_frame_deque.is_empty() {
        instantaneous_frame_deque.push_back(create_blank_dvs_frame(&meta)?);
    }

    let mut event_count_mat = instantaneous_frame_deque[0].clone();
    for y in 0..meta.plane.h_usize() {
        for x in 0..meta.plane.w_usize() {
            for c in 0..meta.plane.c_usize() {
                event_count_mat[[y, x, c]] =
                    ((event_counts[[y, x, c]] as f32 / max_px_event_count as f32) * 255.0) as u8;
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

    if frame_idx >= frame_count {
        frames[frame_idx - frame_count][[event.coord.y.into(), event.coord.x.into(), 0]] =
            value as u8;
        frames[frame_idx - frame_count][[event.coord.y.into(), event.coord.x.into(), 1]] =
            value as u8;
        frames[frame_idx - frame_count][[event.coord.y.into(), event.coord.x.into(), 2]] =
            value as u8;
    }
    Ok(())
}

fn event_to_frame_intensity(event: &Event, frame_length: u128) -> f64 {
    if event.d == D_ZERO_INTEGRATION {
        return 0.0;
    }
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

fn fire_dvs_event(
    polarity: bool,
    x: PixelAddress,
    y: PixelAddress,
    t: u128,
    writer: &mut BufWriter<File>,
    ordered_event_queue: &mut Option<VecDeque<DvsEvent>>,
    write_mode: WriteMode,
) -> io::Result<()> {
    match write_mode {
        WriteMode::Text => {
            let polarity_string = if polarity { "1" } else { "0" };

            let dvs_string = t.to_string()
                + " "
                + x.to_string().as_str()
                + " "
                + y.to_string().as_str()
                + " "
                + polarity_string
                + "\n";
            let amt = writer.write(dvs_string.as_ref()).expect("Could not write");
            debug_assert_eq!(amt, dvs_string.len());
        }
        WriteMode::Binary => {
            let event = DvsEvent {
                t: (t) as u32,
                x: x as u16,
                y: y as u16,
                p: if polarity { 1 } else { 0 },
            };

            if x == 100 && y == 100 {
                dbg!((event.t, event.p));
            }

            match ordered_event_queue {
                None => {
                    write_event_binary(&event, writer)?;
                }
                Some(queue) => {
                    let index = queue
                        .binary_search_by_key(&event.t, |item| item.t)
                        .unwrap_or_else(|i| i);
                    if index > 0 && index < queue.len() {
                        debug_assert!(event.t <= queue[index].t);
                        // dbg!(queue[index].t, event.t);
                    }
                    queue.insert(index, event);
                    // queue.push_back(event);
                }
            }
        }
    }

    Ok(())
}

fn write_event_binary(event: &DvsEvent, writer: &mut BufWriter<File>) -> io::Result<()> {
    // Write in the .dat spec according to https://docs.prophesee.ai/stable/data/file_formats/dat.html

    let mut buffer = [0; 8];

    // t as u32 into the first four bytes of the buffer
    buffer[0..4].copy_from_slice(&(event.t as u32).to_le_bytes());

    let mut data: u32 = 0;

    // polarity as the 4th bit
    data |= (event.p as u32) << 28; // polarity ending at 4th bit from left

    data |= (event.y as u32) << 14; // y ending at 18th bit from left

    data |= event.x as u32; // x ending at 32nd bit from left

    buffer[4..8].copy_from_slice(&data.to_le_bytes());

    let amt = writer.write(&buffer).expect("Could not write");
    debug_assert_eq!(amt, 8);

    Ok(())
}
