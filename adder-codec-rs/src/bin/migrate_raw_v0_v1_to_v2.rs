use adder_codec_rs::transcoder::source::davis::Davis;
use adder_codec_rs::transcoder::source::video::{Source, VideoBuilder};
use aedat::base::ioheader_generated::Compression;
use clap::Parser;
use davis_edi_rs::util::reconstructor::Reconstructor;
use davis_edi_rs::Args as EdiArgs;

use serde::Deserialize;

use adder_codec_rs::raw::stream;
use adder_codec_rs::raw::stream::Error::Eof;
use adder_codec_rs::transcoder::source::davis::TranscoderMode::{Framed, RawDavis, RawDvs};
use adder_codec_rs::SourceCamera::DavisU8;
use adder_codec_rs::{Codec, DeltaT, SourceCamera, TimeMode};
use ndarray::Array3;
use std::io::Write;
use std::path::Path;
use std::time::Instant;
use std::{error, io};

#[derive(Parser, Debug, Deserialize, Default)]
pub struct Args {
    /// Path to input events file
    #[clap(long, default_value = "")]
    pub input_events_filename: String,

    /// Path to output events file
    #[clap(long, default_value = "")]
    pub output_events_filename: String,

    /// Time mode for the v2 file
    #[clap(long, default_value = "")]
    pub time_mode: String,
}

fn main() -> Result<(), Box<dyn error::Error>> {
    let mut args: Args = Args::parse();

    let time_mode = match args.time_mode.to_lowercase().as_str() {
        "delta_t" => TimeMode::DeltaT,
        "absolute" => TimeMode::AbsoluteT,
        "mixed" => TimeMode::Mixed,
        _ => panic!("Invalid time mode"),
    };

    let mut input_stream = stream::Raw::new();
    input_stream.open_reader(Path::new::<String>(&args.input_events_filename.into()))?;
    input_stream.decode_header()?;

    let mut output_stream = stream::Raw::new();
    output_stream.open_writer(Path::new::<String>(&args.output_events_filename.into()))?;
    output_stream.encode_header(
        input_stream.plane.clone(),
        input_stream.tps,
        input_stream.ref_interval.clone(),
        input_stream.delta_t_max,
        2,
        Some(input_stream.source_camera),
        Some(time_mode),
    )?;

    let mut data = Vec::new();
    for _ in 0..input_stream.plane.volume() {
        let t = 0_u32;
        data.push(t);
    }
    let mut t_tree: Array3<u32> = Array3::from_shape_vec(
        (
            input_stream.plane.h_usize(),
            input_stream.plane.w_usize(),
            input_stream.plane.c_usize(),
        ),
        data,
    )?;

    loop {
        let mut event = match input_stream.decode_event() {
            Ok(event) => event,
            Err(_) => {
                break;
            }
        };
        let t = &mut t_tree[[
            event.coord.y_usize(),
            event.coord.x_usize(),
            event.coord.c_usize(),
        ]];

        *t += event.delta_t;

        if output_stream.time_mode == TimeMode::AbsoluteT {
            event.delta_t = *t;

            // If framed video source, we can take advantage of scheme that reduces event rate by half
            if input_stream.codec_version > 0
                && match input_stream.source_camera {
                    SourceCamera::FramedU8
                    | SourceCamera::FramedU16
                    | SourceCamera::FramedU32
                    | SourceCamera::FramedU64
                    | SourceCamera::FramedF32
                    | SourceCamera::FramedF64 => true,
                    SourceCamera::Dvs
                    | SourceCamera::DavisU8
                    | SourceCamera::Atis
                    | SourceCamera::Asint => false,
                }
                && *t % u32::from(input_stream.ref_interval) > 0
            {
                *t = ((*t / u32::from(input_stream.ref_interval)) + 1)
                    * u32::from(input_stream.ref_interval);
            }
        }

        output_stream.encode_event(&event)?;
    }

    output_stream.close_writer()?;
    println!("Done!");
    Ok(())
}
