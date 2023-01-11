

use clap::Parser;

use serde::Deserialize;

use adder_codec_rs::raw::stream;


use adder_codec_rs::utils::stream_migration::migrate_v2;

use adder_codec_rs::{Codec, TimeMode};


use std::path::Path;

use std::{error};

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
    let args: Args = Args::parse();

    let time_mode = match args.time_mode.to_lowercase().as_str() {
        "delta_t" => TimeMode::DeltaT,
        "absolute" => TimeMode::AbsoluteT,
        "mixed" => TimeMode::Mixed,
        _ => panic!("Invalid time mode"),
    };

    let mut input_stream = stream::Raw::new();
    input_stream.open_reader(Path::new::<String>(&args.input_events_filename))?;
    input_stream.decode_header()?;

    let mut output_stream = stream::Raw::new();
    output_stream.open_writer(Path::new::<String>(&args.output_events_filename))?;
    output_stream.encode_header(
        input_stream.plane.clone(),
        input_stream.tps,
        input_stream.ref_interval,
        input_stream.delta_t_max,
        2,
        Some(input_stream.source_camera),
        Some(time_mode),
    )?;

    output_stream = migrate_v2(input_stream, output_stream)?;

    output_stream.close_writer()?;
    println!("Done!");
    Ok(())
}
