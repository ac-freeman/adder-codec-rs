use crate::framer::event_framer::FramerMode::INSTANTANEOUS;
use crate::framer::event_framer::SourceType::U8;
use crate::framer::event_framer::{Framer, FramerBuilder};
use crate::framer::scale_intensity;
use crate::framer::scale_intensity::FrameValue;
use crate::transcoder::source::framed_source::FramedSource;
use crate::transcoder::source::video::Source;
use crate::SourceCamera::FramedU8;
use crate::{DeltaT, Event};
use clap::Parser;
use rayon::ThreadPool;
use serde::Serialize;
use std::cmp::max;
use std::error::Error;
use std::fs::File;
use std::io;
use std::io::{BufWriter, Write};

use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::Instant;

/// Command line argument parser
#[derive(Parser, Debug, Default, serde::Deserialize)]
#[clap(author, version, about, long_about = None)]
pub struct SimulProcArgs {
    /// Filename for args (optional; must be in .toml format)
    #[clap(short, long, default_value = "")]
    pub args_filename: String,

    /// Use color? (For framed input, most likely)
    #[clap(long, action)]
    pub color_input: bool,

    /// Number of ticks per input frame // TODO: modularize for different sources
    #[clap(short, long, default_value_t = 255)]
    pub ref_time: u32,

    /// Max number of ticks for any event
    #[clap(short, long, default_value_t = 15300)]
    pub delta_t_max: u32,

    /// Max number of input frames to transcode (0 = no limit)
    #[clap(short, long, default_value_t = 0)]
    pub frame_count_max: u32,

    /// Index of first input frame to transcode
    #[clap(long, default_value_t = 0)]
    pub frame_idx_start: u32,

    /// Show live view displays?
    #[clap(short, long, action)]
    pub show_display: bool,

    /// Path to input file
    #[clap(short, long, default_value = "./in.mp4")]
    pub input_filename: String,

    /// Path to output events file
    #[clap(long, default_value = "")]
    pub output_events_filename: String,

    /// Path to output raw video file
    #[clap(short, long, default_value = "./out")]
    pub output_raw_video_filename: String,

    /// Resize scale
    #[clap(short('z'), long, default_value_t = 1.0)]
    pub scale: f64,

    /// Positive contrast threshold, in intensity units. How much an intensity must increase
    /// to create a frame division. Only used when look_ahead = 1 and framed input
    #[clap(long, default_value_t = 5)]
    pub c_thresh_pos: u8,

    /// Negative contrast threshold, in intensity units. How much an intensity must decrease
    /// to create a frame division.  Only used when look_ahead = 1 and framed input
    #[clap(long, default_value_t = 5)]
    pub c_thresh_neg: u8,

    /// Number of threads to use. If not provided, will default to the number of cores on the
    /// system.
    #[clap(long, default_value_t = 4)]
    pub thread_count: u8,
}

pub struct SimulProcessor {
    pub source: FramedSource,
    thread_pool: ThreadPool,
    events_tx: Sender<Vec<Vec<Event>>>,
}

impl SimulProcessor {
    pub fn new<T>(
        source: FramedSource,
        ref_time: DeltaT,
        output_path: &str,
        frame_max: i32,
        num_threads: usize,
    ) -> SimulProcessor
    where
        T: Clone + std::marker::Sync + std::marker::Send + 'static,
        T: scale_intensity::FrameValue,
        T: std::default::Default,
        T: std::marker::Copy,
        T: FrameValue<Output = T>,
        T: Serialize,
        T: num_traits::Zero,
    {
        let thread_pool_framer = rayon::ThreadPoolBuilder::new()
            .num_threads(max(num_threads / 2, 1))
            .build()
            .unwrap();
        let thread_pool_transcoder = rayon::ThreadPoolBuilder::new()
            .num_threads(max(num_threads / 2, 1))
            .build()
            .unwrap();
        let reconstructed_frame_rate = source.source_fps;
        // For instantaneous reconstruction, make sure the frame rate matches the source video rate
        assert_eq!(source.video.tps / ref_time, reconstructed_frame_rate as u32);

        let height = source.get_video().height as usize;
        let width = source.get_video().width as usize;
        let channels = source.get_video().channels;

        let mut framer = thread_pool_framer.install(|| {
            FramerBuilder::new(height, width, channels, source.video.chunk_rows)
                .codec_version(1)
                .time_parameters(
                    source.video.tps,
                    ref_time,
                    source.video.delta_t_max,
                    reconstructed_frame_rate,
                )
                .mode(INSTANTANEOUS)
                .source(U8, FramedU8)
                .finish::<T>()
        });

        let mut output_stream = BufWriter::new(File::create(output_path).unwrap());

        let (events_tx, events_rx): (Sender<Vec<Vec<Event>>>, Receiver<Vec<Vec<Event>>>) =
            channel();
        let mut now = Instant::now();

        // Spin off a thread for managing the input frame buffer. It will keep the buffer filled,
        // and pre-process the next input frame (grayscale conversion and rescaling)
        rayon::spawn(move || {
            let mut frame_count = 1;
            loop {
                match events_rx.recv() {
                    Ok(events) => {
                        // assert_eq!(events.len(), (self.source.get_video().height as f64 / self.framer.chunk_rows as f64).ceil() as usize);

                        // Frame the events
                        if framer.ingest_events_events(events) {
                            match framer.write_multi_frame_bytes(&mut output_stream) {
                                0 => {
                                    panic!("Should have frame, but didn't")
                                }
                                frames_returned => {
                                    frame_count += frames_returned;
                                    print!(
                                        "\rOutput frame {}. Got {} frames in  {} ms/frame\t",
                                        frame_count,
                                        frames_returned,
                                        now.elapsed().as_millis() / frames_returned as u128
                                    );
                                    io::stdout().flush().unwrap();
                                    now = Instant::now();
                                }
                            }
                        }
                        output_stream
                            .flush()
                            .expect("Could not flush raw video writer");
                        if frame_count >= frame_max && frame_max > 0 {
                            eprintln!("Wrote max frames. Exiting channel.");
                            break;
                        }
                    }
                    Err(_) => {
                        eprintln!("Event receiver is closed. Exiting channel.");
                        break;
                    }
                };
            }
        });

        SimulProcessor {
            source,
            thread_pool: thread_pool_transcoder,
            events_tx,
        }
    }

    pub fn run(&mut self) -> Result<(), Box<dyn Error>> {
        let mut now = Instant::now();

        loop {
            match self.source.consume(1, &self.thread_pool) {
                Ok(events) => {
                    match self.events_tx.send(events) {
                        Ok(_) => {}
                        Err(_) => {
                            break;
                        }
                    };
                }
                Err(e) => {
                    println!("Err: {:?}", e);
                    break;
                }
            };

            let video = self.source.get_video();

            if video.in_interval_count % 30 == 0 {
                print!(
                    "\rFrame {} in  {}ms",
                    video.in_interval_count,
                    now.elapsed().as_millis()
                );
                io::stdout().flush().unwrap();
                now = Instant::now();
            }
        }

        println!("Closing stream...");
        self.source.get_video_mut().end_write_stream();
        println!("FINISHED");

        Ok(())
    }
}
