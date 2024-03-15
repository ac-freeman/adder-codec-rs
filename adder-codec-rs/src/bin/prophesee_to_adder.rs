use adder_codec_core::codec::rate_controller::{Crf, DEFAULT_CRF_QUALITY};
use adder_codec_core::codec::{EncoderOptions, EncoderType};
use adder_codec_core::SourceCamera::Dvs;
use adder_codec_core::{PixelMultiMode, PlaneSize, TimeMode};
use adder_codec_rs::transcoder::source::prophesee::Prophesee;
use adder_codec_rs::transcoder::source::video::{Source, SourceError, VideoBuilder};
use adder_codec_rs::utils::simulproc::SimulProcArgs;
use adder_codec_rs::utils::viz::ShowFeatureMode;
use clap::Parser;
use rayon::current_num_threads;
use std::fs::File;
use std::io::BufWriter;

#[derive(Parser, Debug, Default, serde::Deserialize)]
#[clap(author, version, about, long_about = None)]
pub struct MyArgs {
    /// Number of ticks per input interval
    #[clap(short, long, default_value_t = 1)]
    pub ref_time: u32,

    /// Max number of ticks for first event at a new intensity
    #[clap(short, long, default_value_t = 2)]
    pub delta_t_max: u32,

    /// Path to input file
    #[clap(short, long, default_value = "./in.dat")]
    pub input: String,

    /// Path to output events file
    #[clap(long, default_value = "")]
    pub output: String,

    #[clap(long, default_value_t = 3)]
    pub crf: u8,

    /// Number of threads to use. If not provided, will default to the number of cores on the
    /// system.
    #[clap(long, default_value_t = 8)]
    pub thread_count: u8,

    #[clap(short, long, action)]
    pub features: bool,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args: MyArgs = MyArgs::parse();

    let mut prophesee_source: Prophesee<BufWriter<File>> =
        Prophesee::new(args.ref_time, args.input)?.crf(args.crf);
    let adu_interval =
        (prophesee_source.get_video_ref().state.tps as f32 / args.ref_time as f32) as usize;
    let plane = prophesee_source.get_video_ref().state.plane;

    let writer = BufWriter::new(File::create(args.output)?);
    prophesee_source = *prophesee_source.write_out(
        Dvs,
        TimeMode::AbsoluteT,
        PixelMultiMode::Collapse,
        Some(adu_interval),
        EncoderType::Compressed,
        EncoderOptions {
            event_drop: Default::default(),
            event_order: Default::default(),
            crf: Crf::new(Some(args.crf), plane),
        },
        writer,
    )?;
    prophesee_source
        .get_video_mut()
        .update_detect_features(args.features, ShowFeatureMode::Off, true, true);
    // prophesee_source
    //     .get_video_mut()
    //     .encoder
    //     .options
    //     .crf
    //     .override_feature_c_radius(2);

    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(args.thread_count.into())
        .build()
        .unwrap();

    loop {
        match prophesee_source.consume( &pool) {
            Ok(_) => {}
            Err(SourceError::Open) => return Ok(()),
            Err(e) => {
                eprintln!("Consume Error: {:?}", e);
                prophesee_source.get_video_mut().end_write_stream()?;
                return Ok(());
            }
        };
    }

    Ok(())
}
