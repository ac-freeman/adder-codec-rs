/*
Created on 10/19/23 to evaluate feature detection speed & accuracy, and CRF quality.

Example usage:
cargo run --bin evaluate_feature_detection_transcode --release --features "open-cv feature-logging" -- --crf 6 --delta-t-max 7650 --frame-count-max 500 --input-filename "/home/andrew/Downloads/bunny/bunny.mp4" --scale 0.25 --detect-features

 */
extern crate core;

use adder_codec_rs::transcoder::source::video::{
    FramedViewMode, Source, SourceError, VideoBuilder,
};

use clap::Parser;
use indicatif::ProgressBar;
use rayon::current_num_threads;
use std::error::Error;
use std::fs::File;
use std::io::{BufReader, Write};

use adder_codec_core::codec::decoder::Decoder;
use adder_codec_core::codec::rate_controller::{Crf, DEFAULT_CRF_QUALITY};
use adder_codec_core::codec::{EncoderOptions, EncoderType};
use adder_codec_core::TimeMode::AbsoluteT;
use adder_codec_core::{open_file_decoder, Event, Intensity, SourceCamera, TimeMode};
use adder_codec_rs::framer::driver::FramerMode::INSTANTANEOUS;
use adder_codec_rs::framer::driver::{FrameSequence, Framer, FramerBuilder};
use adder_codec_rs::transcoder::source::framed::Framed;
use adder_codec_rs::utils::cv::{calculate_quality_metrics, handle_color, QualityMetrics};
use adder_codec_rs::utils::viz::ShowFeatureMode::Off;
use bitstream_io::{BigEndian, BitReader};
use ndarray::{Array3, ArrayBase, Ix3, OwnedRepr};
use std::io::{BufWriter, Cursor};
use std::path::{Path, PathBuf};
use video_rs::{Locator, Options, Resize};

/// Command line argument parser
#[derive(Parser, Debug, Default, serde::Deserialize)]
#[clap(author, version, about, long_about = None)]
pub struct TranscodeFeatureEvalArgs {
    /// Use color? (For framed input, most likely)
    #[clap(long, action)]
    pub color_input: bool,

    /// Perform source-modeled compression?
    #[clap(long, action)]
    pub compressed: bool,

    /// Path to output file
    #[clap(short, long, default_value = "")]
    pub output_filename: String,

    /// CRF quality. 0 = lossless, 9 = worst quality
    #[clap(short, long, default_value_t = 6)]
    pub crf: u8,

    /// Run feature detection?
    #[clap(long, action)]
    pub detect_features: bool,

    /// Max number of ticks before a pixel fires its FIRST event
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
    #[clap(long, default_value_t = 0)]
    pub thread_count: u8,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let args: TranscodeFeatureEvalArgs = TranscodeFeatureEvalArgs::parse();

    let num_threads = match args.thread_count {
        0 => current_num_threads(),
        num => num as usize,
    };
    eprintln!("Using {} threads", num_threads);

    let now = std::time::Instant::now();

    let path = PathBuf::from(args.input_filename.clone());

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
                .detect_features(args.detect_features, Off)
                .log_path(format!(
                    "{}_{}_{}_",
                    args.crf,
                    if args.compressed { "compressed" } else { "raw" },
                    path.file_stem().unwrap().to_str().unwrap().to_string()
                ));

                if args.output_filename.len() > 0 {
                    let mut options = EncoderOptions::default(framed.get_video_ref().state.plane);

                    let plane = framed.get_video_ref().state.plane;
                    options.crf = Crf::new(Some(args.crf), plane);

                    framed = framed
                        .write_out(
                            SourceCamera::FramedU8,
                            AbsoluteT,
                            if args.compressed {
                                EncoderType::Compressed
                            } else {
                                EncoderType::Raw
                            },
                            options,
                            BufWriter::new(File::create(args.output_filename.clone())?),
                        )?
                        .chunk_rows(plane.h_usize());
                }

                Ok(framed)
            }

            Some(_) => Err("Invalid file type"),
        },
    }?;

    #[cfg(feature = "feature-logging")]
    {
        let video = &mut source.get_video_mut();
        let parameters = video.encoder.options.crf.get_parameters();
        let quality = video.encoder.options.crf.get_quality();
        // Write the plane size to the log file
        if let Some(handle) = &mut video.state.feature_log_handle {
            writeln!(handle, "META: Ticks per second: {}", video.state.tps)?;
            writeln!(
                handle,
                "META: Reference ticks per source interval: {}",
                video.state.ref_time
            )?;
            writeln!(handle, "META: Δt_max: {}", video.state.delta_t_max)?;
            writeln!(
                handle,
                "META: CRF: {}",
                quality.unwrap_or(DEFAULT_CRF_QUALITY)
            )?;
            writeln!(
                handle,
                "META: c_thresh_baseline: {}",
                parameters.c_thresh_baseline
            )?;
            writeln!(handle, "META: c_thresh_max: {}", parameters.c_thresh_max)?;
            writeln!(
                handle,
                "META: c_increase_velocity: {}",
                parameters.c_increase_velocity
            )?;
        }
    }

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
    match source.get_video_mut().end_write_stream() {
        Ok(Some(mut writer)) => {
            writer.flush();
        }
        Ok(None) => {}
        Err(_) => {}
    }

    println!("\n\n{} ms elapsed\n\n", now.elapsed().as_millis());

    #[cfg(feature = "feature-logging")]
    {
        // Reconstruct the video to determine what loss is from transcoder vs source-modeled compression
        if args.output_filename.len() > 0 {
            // Setup another input framed video reader
            let orig_source = Locator::Path(PathBuf::from(args.input_filename));
            let mut cap = video_rs::Decoder::new(&orig_source)?;
            let (width, height) = cap.size();
            let width = ((width as f64) * args.scale) as u32;
            let height = ((height as f64) * args.scale) as u32;

            cap = video_rs::Decoder::new_with_options_and_resize(
                &orig_source,
                &Options::default(),
                Resize::Fit(width, height),
            )?;
            let video_frame_count = cap.frame_count();
            if args.frame_idx_start >= video_frame_count as u32 {
                return Err(Box::try_from("Start idx out of bounds").unwrap());
            };
            let source_fps = cap.frame_rate();
            let ts_millis = (args.frame_idx_start as f32 / source_fps * 1000.0) as i64;
            cap.reader.seek(ts_millis)?;

            // Setup the addder video reader
            let (mut stream, mut bitreader) = open_file_decoder(&args.output_filename)?;

            let meta = *stream.meta();
            let mut reconstructed_frame_rate = (meta.tps / meta.ref_interval) as f32;

            let framer_builder: FramerBuilder = FramerBuilder::new(meta.plane, 260)
                .codec_version(meta.codec_version, meta.time_mode)
                .time_parameters(
                    meta.tps,
                    meta.ref_interval,
                    meta.delta_t_max,
                    reconstructed_frame_rate,
                )
                .mode(INSTANTANEOUS)
                .view_mode(FramedViewMode::Intensity)
                .detect_features(false)
                .source(stream.get_source_type(), meta.source_camera);

            let mut frame_sequence: FrameSequence<u8> = framer_builder.clone().finish();

            let video = &mut source.get_video_mut();
            if let Some(handle) = &mut video.state.feature_log_handle {
                eprintln!("Reconstructing");
                let out = format!("\nRECONSTRUCTION\n");
                handle
                    .write_all(&serde_pickle::to_vec(&out, Default::default()).unwrap())
                    .unwrap();

                let pb = ProgressBar::new(args.frame_count_max.into());
                loop {
                    let (_, frame) = cap.decode()?;
                    let input_frame = handle_color(frame, args.color_input)?;

                    let (event_count, mut recon_image) = reconstruct_frame_from_adder(
                        &mut frame_sequence,
                        &mut stream,
                        &mut bitreader,
                    )
                    .unwrap();
                    let mut recon_image = match recon_image {
                        None => {
                            println!("Finished");
                            return Ok(());
                        }
                        Some(a) => a,
                    };
                    // Get the quality metrics compared to the source video
                    #[rustfmt::skip]
                        let metrics = calculate_quality_metrics(
                        &input_frame,
                        &mut recon_image,
                        QualityMetrics {
                            mse: Some(0.0),
                            psnr: Some(0.0),
                            ssim: Some(0.0),
                        },
                    );
                    let metrics = metrics.unwrap();
                    let bytes = serde_pickle::to_vec(&metrics, Default::default()).unwrap();
                    handle.write_all(&bytes).unwrap();
                    pb.inc(1);
                }
            }
        }
    }

    Ok(())
}

fn reconstruct_frame_from_adder(
    frame_sequence: &mut FrameSequence<u8>,
    stream: &mut Decoder<BufReader<File>>,
    bitreader: &mut BitReader<BufReader<File>, BigEndian>,
) -> Result<(i32, Option<ArrayBase<OwnedRepr<u8>, Ix3>>), Box<dyn Error>> {
    let image = if frame_sequence.is_frame_0_filled() {
        let mut db = Array3::zeros((
            stream.meta().plane.h_usize(),
            stream.meta().plane.w_usize(),
            stream.meta().plane.c_usize(),
        ));
        let new_frame = frame_sequence.pop_next_frame().unwrap();
        let mut y: usize = 0;
        let mut x: usize = 0;
        let mut c: usize = 0;
        for chunk in new_frame {
            for px in chunk.iter() {
                match px {
                    Some(event) => unsafe {
                        *db.uget_mut((y, x, c)) = *event;
                        c += 1;
                        if c == stream.meta().plane.c_usize() {
                            c = 0;
                            x += 1;
                            if x == stream.meta().plane.w_usize() {
                                x = 0;
                                y += 1;
                            }
                        }
                    },
                    None => {}
                };
            }
        }

        Some(db)
    } else {
        None
    };

    if image.is_some() {
        return Ok((0, image));
    }

    let mut event_count = 0;
    let mut last_event: Option<Event> = None;
    loop {
        match stream.digest_event(bitreader) {
            Ok(mut event) => {
                event_count += 1;
                let filled = frame_sequence.ingest_event(&mut event, last_event);

                last_event = Some(event.clone());

                if filled {
                    match image {
                        None => {
                            return reconstruct_frame_from_adder(frame_sequence, stream, bitreader);
                        }
                        Some(image) => {
                            return Ok((event_count, Some(image)));
                        }
                    }
                }
            }
            Err(e) => {
                eprintln!("Player error: {}", e);
                if !frame_sequence.flush_frame_buffer() {
                    eprintln!("Completely done");

                    return Ok((event_count, image));
                } else {
                    return reconstruct_frame_from_adder(frame_sequence, stream, bitreader);
                }
            }
        }
    }
}
