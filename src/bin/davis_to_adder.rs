use adder_codec_rs::transcoder::source::davis_source::DavisSource;
use adder_codec_rs::transcoder::source::video::Source;
use aedat::base::ioheader_generated::Compression;
use clap::Parser;
use davis_edi_rs::util::reconstructor::Reconstructor;
use davis_edi_rs::Args as EdiArgs;

use serde::Deserialize;

use adder_codec_rs::transcoder::source::davis_source::DavisTranscoderMode::{Framed, Raw};
use std::any::Any;
use std::io::Write;
use std::time::Instant;
use std::{error, io};

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

    /// Positive contrast threshold, in intensity units. How much an intensity must increase
    /// to launch a D-value reset.
    #[clap(long, default_value_t = 5)]
    pub adder_c_thresh_pos: u8,

    /// Negative contrast threshold, in intensity units. How much an intensity must decrease
    /// to launch a D-value reset.
    #[clap(long, default_value_t = 5)]
    pub adder_c_thresh_neg: u8,

    /// Multiplier for max number of ticks for any event. delta_t_max := (ticks per second) * (this
    /// multiplier)
    #[clap(short, long, default_value_t = 1.0)]
    pub delta_t_max_multiplier: f64,

    /// Transcode from the framed video of DAVIS reconstruction, or from deblurred APS frames and
    /// raw DVS events? (options are "framed", "raw")
    #[clap(short, long, default_value = "")]
    pub transcode_from: String,
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
    let args = args;

    // let transcode_type = match args.transcode_from.as_str() {
    //     "raw" => Raw,
    //     _ => Framed,
    // };

    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(12)
        .build()
        .unwrap();
    let reconstructor = rt.block_on(Reconstructor::new(
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
        edi_args.deblur_only != 0,
        edi_args.target_latency,
    ));

    let mode = match args.transcode_from.as_str() {
        "raw" => Raw,
        _ => Framed,
    };

    let mut davis_source = DavisSource::new(
        reconstructor,
        Some(args.output_events_filename),
        (1000000) as u32,                                 // TODO
        (1000000.0 * args.delta_t_max_multiplier) as u32, // TODO
        args.show_display != 0,
        args.adder_c_thresh_pos,
        args.adder_c_thresh_neg,
        rt,
        mode,
    )
    .unwrap();

    let mut now = Instant::now();
    let thread_pool_integration = rayon::ThreadPoolBuilder::new()
        .num_threads(4)
        .build()
        .unwrap();

    loop {
        match davis_source.consume(1, &thread_pool_integration) {
            Ok(_events) => {}
            Err(e) => {
                println!("Err: {:?}", e);
                break;
            }
        };
        davis_source.integrate_dvs_events();
        if davis_source.get_video().in_interval_count % 30 == 0 {
            println!(
                "\rDavis recon frame to ADDER {} in  {}ms",
                davis_source.get_video().in_interval_count,
                now.elapsed().as_millis()
            );
            io::stdout().flush().unwrap();
            now = Instant::now();
        }
    }

    Ok(())
}
