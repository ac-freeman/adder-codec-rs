use adder_codec_rs::transcoder::source::davis_source::DavisSource;
use adder_codec_rs::transcoder::source::video::Source;
use aedat::base::ioheader_generated::Compression;
use clap::Parser;
use davis_edi_rs::util::reconstructor::Reconstructor;
use davis_edi_rs::Args as EdiArgs;
use opencv::core::Mat;
use serde::Deserialize;
use std::fs::File;
use std::io::BufWriter;
use std::time::Instant;
use std::{error, io};
use tokio::io::AsyncBufRead;

#[derive(Parser, Debug, Deserialize, Default)]
pub struct Args {
    /// Filename for EDI args (optional; must be in .toml format)
    #[clap(short, long, default_value = "")]
    pub edi_args_filename: String,

    /// Filename for args (optional; must be in .toml format)
    #[clap(short, long, default_value = "")]
    pub args_filename: String,

    /// Path to output events file
    #[clap(long, default_value = "")]
    pub output_events_filename: String,

    /// Show live view displays? (1=yes,0=no)
    #[clap(short, long, default_value_t = 0)]
    pub show_display: u32,
}

fn main() -> Result<(), Box<dyn error::Error>> {
    let mut args: Args = Args::parse();
    if !args.args_filename.is_empty() {
        let content = std::fs::read_to_string(args.args_filename)?;
        args = toml::from_str(&content).unwrap();
    }

    let mut edi_args: EdiArgs = EdiArgs::parse();
    if !args.edi_args_filename.is_empty() {
        let content = std::fs::read_to_string(args.edi_args_filename)?;
        edi_args = toml::from_str(&content).unwrap();
    }

    let mut args: Args = Args::parse();
    if !args.args_filename.is_empty() {
        let content = std::fs::read_to_string(args.args_filename)?;
        args = toml::from_str(&content).unwrap();
    }

    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(12)
        .build()
        .unwrap();
    let mut reconstructor = rt.block_on(Reconstructor::new(
        edi_args.base_path,
        edi_args.events_filename_0,
        edi_args.events_filename_1,
        edi_args.mode,
        edi_args.start_c,
        edi_args.optimize_c != 0,
        edi_args.optimize_controller != 0,
        edi_args.show_display != 0,
        edi_args.show_blurred_display != 0,
        edi_args.output_fps,
        Compression::None,
        346,
        260,
    ));

    let mut davis_source = DavisSource::new(
        reconstructor,
        Some(args.output_events_filename),
        (edi_args.output_fps * 5000.0) as u32,
        (edi_args.output_fps * 5000.0) as u32,
        args.show_display != 0,
        rt,
    )
    .unwrap();

    let mut now = Instant::now();
    let thread_pool_integration = rayon::ThreadPoolBuilder::new()
        .num_threads(4)
        .build()
        .unwrap();

    loop {
        match davis_source.consume(1, &thread_pool_integration) {
            Ok(events) => {}
            Err(e) => {
                println!("Err: {:?}", e);
                break;
            }
        };
    }

    Ok(())
}
