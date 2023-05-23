use adder_codec_rs::transcoder::source::davis::Davis;
use adder_codec_rs::transcoder::source::video::{Source, VideoBuilder};
use clap::Parser;
use davis_edi_rs::util::reconstructor::Reconstructor;
use davis_edi_rs::Args as EdiArgs;

use serde::Deserialize;

use adder_codec_core::DeltaT;

use adder_codec_core::codec::EncoderType;
use adder_codec_core::SourceCamera::DavisU8;
use adder_codec_core::TimeMode;
use adder_codec_rs::transcoder::source::davis::TranscoderMode::{Framed, RawDavis, RawDvs};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::time::Instant;
use std::{error, io};

#[derive(Parser, Debug, Deserialize, Default)]
pub struct Args {
    /// Filename for EDI args (optional; must be in .toml format)
    /// OR can provide toml-style data as a raw string here
    #[clap(short, long, default_value = "")]
    pub edi_args: String,

    /// Filename for args (optional; must be in .toml format)
    #[clap(short, long, default_value = "")]
    pub args_filename: String,

    /// Path to output events file
    #[clap(long, default_value = "")]
    pub output_events_filename: String,

    /// Show live view displays?
    #[clap(short, long, action)]
    pub show_display: bool,

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

    /// Optimize the ADΔER controller for latency?
    /// If true, then the ADΔER transcoder will attempt to maintain the maximum latency as defined
    /// for the EDI reconstructor, by adjusting the ADΔER contrast threshold (and thus the ADΔER
    /// event rate).
    #[clap(long, action)]
    pub optimize_adder_controller: bool,

    /// Write out ADΔER file?
    #[clap(short, long, action)]
    pub write_out: bool,
}

#[allow(dead_code)]
fn main() -> Result<(), Box<dyn error::Error>> {
    let mut args: Args = Args::parse();
    if !args.args_filename.is_empty() {
        let content = std::fs::read_to_string(args.args_filename)?;
        args = toml::from_str(&content)?;
    }

    let mut edi_args: EdiArgs = EdiArgs::default();
    if !args.edi_args.is_empty() {
        match std::fs::read_to_string(&args.edi_args) {
            Ok(content) => {
                edi_args = toml::from_str(&content)?;
            }
            Err(_) => {
                edi_args = toml::from_str(&args.edi_args)?;
            }
        };
    }

    if args.optimize_adder_controller {
        // assert!(edi_args.optimize_controller);
    }

    // let transcode_type = match args.transcode_from.as_str() {
    //     "raw" => Raw,
    //     _ => Framed,
    // };
    let mode = match args.transcode_from.as_str() {
        "raw-davis" => RawDavis,
        "raw-dvs" => RawDvs,
        _ => Framed,
    };

    let events_only = match mode {
        Framed => false,
        RawDavis => false,
        RawDvs => true,
    };

    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(12)
        .enable_time()
        .build()?;
    let reconstructor = rt.block_on(Reconstructor::new(
        edi_args.base_path,
        edi_args.events_filename_0,
        edi_args.events_filename_1,
        edi_args.mode,
        edi_args.start_c,
        edi_args.optimize_c,
        edi_args.optimize_c_frequency,
        edi_args.optimize_controller,
        edi_args.show_display,
        edi_args.show_blurred_display,
        edi_args.output_fps,
        edi_args.deblur_only,
        events_only,
        edi_args.target_latency,
        edi_args.simulate_packet_latency,
    ))?;

    let file = File::create(args.output_events_filename)?;
    let writer = BufWriter::new(file);
    let ref_time = (1_000_000.0 / edi_args.output_fps) as DeltaT;

    let mut davis_source = Box::new(
        Davis::<BufWriter<File>>::new(reconstructor, rt, mode)?
            .optimize_adder_controller(args.optimize_adder_controller)
            .mode(mode)
            .time_parameters(
                1_000_000, // TODO
                ref_time,
                (ref_time as f64 * args.delta_t_max_multiplier) as u32,
                Some(TimeMode::AbsoluteT),
            )? // TODO
            .c_thresh_pos(args.adder_c_thresh_pos)
            .c_thresh_neg(args.adder_c_thresh_neg),
    )
    .write_out(DavisU8, TimeMode::AbsoluteT, EncoderType::Raw, writer)?;

    let mut now = Instant::now();
    let start_time = std::time::Instant::now();
    let thread_pool_integration = rayon::ThreadPoolBuilder::new().num_threads(1).build()?;

    loop {
        match davis_source.consume(1, &thread_pool_integration) {
            Ok(_events) => {}
            Err(e) => {
                println!("Err: {e:?}");
                break;
            }
        };

        if davis_source.get_video_ref().state.in_interval_count % 30 == 0 {
            println!(
                "\rDavis recon frame to ADΔER {} in  {}ms",
                davis_source.get_video_ref().state.in_interval_count,
                now.elapsed().as_millis()
            );
            io::stdout().flush()?;
            now = Instant::now();
        }
    }

    println!("\n\n{} ms elapsed\n\n", start_time.elapsed().as_millis());

    Ok(())
}
