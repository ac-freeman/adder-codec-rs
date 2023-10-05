use adder_codec_core::codec::decoder::Decoder;
use adder_codec_core::codec::encoder::Encoder;
use adder_codec_core::codec::raw::stream::{RawInput, RawOutput};
use adder_codec_core::codec::EncoderOptions;
use adder_codec_core::TimeMode;
use adder_codec_rs::utils::stream_migration::migrate_v2;
use bitstream_io::{BigEndian, BitReader};
use clap::Parser;
use serde::Deserialize;
use std::error;
use std::fs::File;
use std::io::{BufReader, BufWriter};

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

    let tmp = File::open(args.input_events_filename).unwrap();
    let bufreader = BufReader::new(tmp);
    let compression = RawInput::new();

    let mut bitreader = BitReader::endian(bufreader, BigEndian);
    let input_stream = Decoder::new_raw(compression, &mut bitreader).unwrap();

    let bufwriter = BufWriter::new(File::create(args.output_events_filename).unwrap());
    let mut new_meta = *input_stream.meta();
    new_meta.time_mode = time_mode;
    let compression = RawOutput::new(new_meta, bufwriter);
    let mut encoder: Encoder<BufWriter<File>> =
        Encoder::new_raw(compression, EncoderOptions::default());

    encoder = migrate_v2(input_stream, &mut bitreader, encoder)?;

    encoder.close_writer()?;
    println!("Done!");
    Ok(())
}
