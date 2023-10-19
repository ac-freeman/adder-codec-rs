/*
Created on 10/19/23 to evaluate feature detection speed & accuracy, and CRF quality.
 */
extern crate core;

use adder_codec_rs::transcoder::source::video::{Source, SourceError, VideoBuilder};

use clap::Parser;
use rayon::current_num_threads;

use indicatif::ProgressBar;
use std::error::Error;
use std::fs::File;

use adder_codec_core::codec::{EncoderOptions, EncoderType};
use adder_codec_core::SourceCamera::FramedU8;
use adder_codec_core::TimeMode;
use adder_codec_rs::transcoder::source::framed::Framed;
use adder_codec_rs::utils::viz::ShowFeatureMode::Off;
use std::io::{BufWriter, Cursor};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Command line argument parser
#[derive(Parser, Debug, Default, serde::Deserialize)]
#[clap(author, version, about, long_about = None)]
pub struct TranscodeFeatureEvalArgs {
    /// Use color? (For framed input, most likely)
    #[clap(long, action)]
    pub color_input: bool,

    /// CRF quality. 0 = lossless, 9 = worst quality
    #[clap(short, long, default_value_t = 6)]
    pub crf: u8,

    /// Run feature detection?
    #[clap(long, action)]
    pub detect_features: bool,

    /// Override what the CRF setting determines: Max number of ticks for any event
    #[clap(short, long, default_value_t = 15300)]
    pub delta_t_max: u32,

    /// Max number of input frames to transcode (0 = no limit)
    #[clap(short, long, default_value_t = 0)]
    pub frame_count_max: u32,

    /// Index of first input frame to transcode
    #[clap(long, default_value_t = 0)]
    pub frame_idx_start: u32,

    /// Path to input file
    #[clap(short, long, default_value = "./in.mp4")]
    pub input_filename: String,

    /// Resize scale
    #[clap(short('z'), long, default_value_t = 1.0)]
    pub scale: f64,

    /// Number of threads to use. If not provided, will default to the number of cores on the
    /// system.
    #[clap(long, default_value_t = 4)]
    pub thread_count: u8,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let mut args: TranscodeFeatureEvalArgs = TranscodeFeatureEvalArgs::parse();

    let num_threads = match args.thread_count {
        0 => current_num_threads(),
        num => num as usize,
    };

    let now = std::time::Instant::now();

    let path = PathBuf::from(args.input_filename);

    let mut source = match path.extension() {
        None => Err("Invalid file type"),
        Some(ext) => match ext.to_ascii_lowercase().to_str() {
            None => Err("Invalid file type"),
            Some("mp4") => {
                let mut framed: Framed<BufWriter<File>> = Framed::new(
                    match path.to_str() {
                        None => return Err("Couldn't get input path string".into()),
                        Some(path) => path.to_string(),
                    },
                    args.color_input,
                    args.scale,
                )?
                .frame_start(args.frame_idx_start)?
                .chunk_rows(64)
                .crf(args.crf)
                .auto_time_parameters(255, args.delta_t_max, Some(TimeMode::AbsoluteT))?
                .show_display(false)
                .detect_features(args.detect_features, Off);

                Ok(framed)
            }

            Some(_) => Err("Invalid file type"),
        },
    }?;

    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(num_threads)
        .build()?;

    let pb = ProgressBar::new(args.frame_count_max.into());
    let mut interval_count = 0;

    while interval_count < args.frame_count_max {
        match source.consume(1, &pool) {
            Ok(events_vec_vec) => {}
            Err(SourceError::Open) => {}
            Err(e) => {
                eprintln!("Error: {:?}", e);
                return Ok(());
            }
        };
        pb.inc(1);
        interval_count += 1;
    }

    println!("\n\n{} ms elapsed\n\n", now.elapsed().as_millis());

    Ok(())
}
