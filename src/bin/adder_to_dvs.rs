use adder_codec_rs::raw::raw_stream::RawStream;
use adder_codec_rs::{Codec, Event};
use std::io::{SeekFrom, Write};
use std::path::Path;
use std::{error, io};

use adder_codec_rs::transcoder::source::video::{show_display, show_display_force};
use clap::Parser;
use ndarray::{Array3, Shape};
use opencv::core::{Mat, MatTrait, MatTraitManual, CV_8U, CV_8UC3};
use std::option::Option;
use tokio::io::AsyncSeekExt;

/// Command line argument parser
#[derive(Parser, Debug, Default)]
#[clap(author, version, about, long_about = None)]
pub struct MyArgs {
    /// Input ADDER video path
    #[clap(short, long)]
    pub(crate) input: String,

    /// Output DVS video path (text file
    #[clap(short, long)]
    pub(crate) output: String,
}

struct DvsPixel {
    d: u8,
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

    loop {
        if event_count % divisor == 0 {
            write!(
                handle,
                "\rTranscoding ADDER to DVS...{}%",
                (event_count * 100) / num_events as u64
            )?;
            handle.flush().unwrap();

            show_display_force("DVS", &instantaneous_frame, 1000 / 30);
            // Clear the instantaneous frame
            match instantaneous_frame.data_bytes_mut() {
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

        match stream.decode_event() {
            Ok(event) => {
                event_count += 1;
                let y = event.coord.y as usize;
                let x = event.coord.x as usize;
                let c = event.coord.c.unwrap_or(0) as usize;
                match &mut pixels[[y, x, c]] {
                    None => {
                        if event.d < 253 {
                            pixels[[y, x, c]] = Some(DvsPixel {
                                d: event.d,
                                t: event.delta_t as u128,
                            })
                        }
                    }
                    Some(px) => {
                        match event.d {
                            255 | 254 => {
                                // ignore empty events
                                px.t += event.delta_t as u128;
                                continue; // Don't update d with this
                            }
                            a if a < px.d.saturating_sub(0) => {
                                // Fire a negative polarity event
                                set_instant_dvs_pixel(event, &mut instantaneous_frame, -10);
                            }
                            a if a > px.d => {
                                // Fire a positive polarity event
                                set_instant_dvs_pixel(event, &mut instantaneous_frame, 20);
                            }
                            _ => {
                                // D is the same. Don't fire an event.
                                set_instant_dvs_pixel(event, &mut instantaneous_frame, 128);
                            }
                        }

                        px.d = event.d;
                        px.t += event.delta_t as u128;
                    }
                }
            }
            Err(_e) => {
                break;
            }
        }
    }

    handle.flush().unwrap();
    println!("\nFinished!");
    Ok(())
}

fn set_instant_dvs_pixel(event: Event, frame: &mut Mat, value: i16) {
    unsafe {
        let px: &mut u8 = frame
            .at_3d_unchecked_mut(
                event.coord.y.into(),
                event.coord.x.into(),
                event.coord.c.unwrap_or(0).into(),
            )
            .unwrap();
        match value {
            128 => *px = 128,
            a => *px = (*px as i16 + a) as u8,
        }
    }
}
