use adder_codec_core::open_file_decoder;
use clap::Parser;
use std::error;

/// Command line argument parser
#[derive(Parser, Debug, Default)]
#[clap(author, version, about, long_about = None)]
pub struct MyArgs {
    /// Input ADΔER video path
    #[clap(short, long)]
    pub(crate) input: String,
}

fn main() -> Result<(), Box<dyn error::Error>> {
    let args: MyArgs = MyArgs::parse();
    let file_path = args.input.as_str();
    let (mut stream, mut bitreader) = open_file_decoder(file_path)?;

    let first_event_position = stream.get_input_stream_position(&mut bitreader)?;

    let meta = *stream.meta();

    stream.set_input_stream_position(&mut bitreader, first_event_position)?;

    // Setup time tracker for AbsoluteT mode
    let data = vec![0_u32; meta.plane.volume()];

    let start_time = std::time::Instant::now();
    while stream.digest_event(&mut bitreader).is_ok() {}
    let duration = start_time.elapsed();
    println!("Time to digest all events: {:?}", duration);

    Ok(())
}
