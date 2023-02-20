use clap::Parser;
use serde::Deserialize;
use std::error;

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
    // let args: Args = Args::parse();
    //
    // let time_mode = match args.time_mode.to_lowercase().as_str() {
    //     "delta_t" => TimeMode::DeltaT,
    //     "absolute" => TimeMode::AbsoluteT,
    //     "mixed" => TimeMode::Mixed,
    //     _ => panic!("Invalid time mode"),
    // };
    //
    // let mut input_stream = stream::Raw::new();
    // let file = File::open(Path::new::<String>(&args.input_events_filename))?;
    // input_stream.set_input_stream(Some(BufReader::new(file)));
    // input_stream.decode_header()?;
    //
    // let mut output_stream = stream::Raw::new();
    // let file = File::create(Path::new::<String>(&args.output_events_filename))?;
    // output_stream.set_output_stream(Some(BufWriter::new(file)));
    // output_stream.encode_header(
    //     input_stream.plane.clone(),
    //     input_stream.tps,
    //     input_stream.ref_interval,
    //     input_stream.delta_t_max,
    //     2,
    //     Some(input_stream.source_camera),
    //     Some(time_mode),
    // )?;
    //
    // output_stream = migrate_v2(input_stream, output_stream)?;
    //
    // output_stream.close_writer()?;
    // println!("Done!");
    Ok(())
}
