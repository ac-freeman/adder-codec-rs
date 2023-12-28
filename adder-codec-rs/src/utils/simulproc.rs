use crate::framer::driver::FramerMode::INSTANTANEOUS;
use crate::framer::driver::{Framer, FramerBuilder};
use crate::framer::scale_intensity;
use crate::framer::scale_intensity::FrameValue;
use crate::transcoder::source::framed::Framed;
use crate::transcoder::source::video::Source;
use adder_codec_core::DeltaT;
use clap::Parser;
use rayon::ThreadPool;
use serde::Serialize;
use std::cmp::max;
use std::error::Error;
use std::fs::File;
use std::io;
use std::io::{BufWriter, Write};

use adder_codec_core::SourceCamera::FramedU8;
use adder_codec_core::SourceType::U8;
use adder_codec_core::{Event, TimeMode};
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

    /// Time mode for the v2 file
    #[clap(long, default_value = "")]
    pub time_mode: String,
}

/// A struct for simultaneously transcoding a video source to ADΔER and reconstructing a framed
/// video from ADΔER
pub struct SimulProcessor<W: Write + 'static> {
    /// Framed transcoder hook
    pub source: Framed<W>,
    thread_pool: ThreadPool,
    events_tx: Sender<Vec<Vec<Event>>>,
}

impl<W: Write + 'static> SimulProcessor<W> {
    /// Create a new SimulProcessor
    ///
    /// # Arguments
    ///
    /// * `source`: [`Framed<W>`] source
    /// * `ref_time`: ticks per source frame
    /// * `output_path`: path to output file
    /// * `frame_max`: max number of frames to transcode
    /// * `num_threads`: number of threads to use
    /// * `codec_version`: codec version
    /// * `time_mode`: time mode
    ///
    /// returns: `Result<SimulProcessor<W>, Box<dyn Error, Global>>`
    ///
    /// # Examples
    /// TODO: add examples
    pub fn new<T>(
        source: Framed<W>,
        ref_time: DeltaT,
        output_path: &str,
        frame_max: i32,
        num_threads: usize,
        codec_version: u8,
        time_mode: TimeMode,
    ) -> Result<SimulProcessor<W>, Box<dyn Error>>
    where
        T: Clone
            + std::marker::Sync
            + std::marker::Send
            + 'static
            + scale_intensity::FrameValue
            + std::default::Default
            + std::marker::Copy
            + FrameValue<Output = T>
            + Serialize
            + num_traits::Zero
            + Into<f64>,
    {
        let thread_pool_framer = rayon::ThreadPoolBuilder::new()
            .num_threads(max(num_threads / 2, 1))
            .build()?;
        let thread_pool_transcoder = rayon::ThreadPoolBuilder::new()
            .num_threads(max(num_threads / 2, 1))
            .build()?;
        let reconstructed_frame_rate = source.source_fps;
        // For instantaneous reconstruction, make sure the frame rate matches the source video rate
        assert_eq!(
            source.video.state.tps / ref_time,
            reconstructed_frame_rate as u32
        );

        let plane = source.get_video_ref().state.plane;

        let mut framer = thread_pool_framer.install(|| {
            FramerBuilder::new(plane, source.video.state.chunk_rows)
                .codec_version(codec_version, time_mode)
                .time_parameters(
                    source.video.state.tps,
                    ref_time,
                    source.video.state.params.delta_t_max,
                    Some(reconstructed_frame_rate),
                )
                .mode(INSTANTANEOUS)
                .source(U8, FramedU8)
                .finish::<T>()
        });

        let mut output_stream = BufWriter::new(File::create(output_path)?);

        let (events_tx, events_rx): (Sender<Vec<Vec<Event>>>, Receiver<Vec<Vec<Event>>>) =
            channel();
        let mut now = Instant::now();

        // Spin off a thread for managing the input frame buffer. It will keep the buffer filled,
        // and pre-process the next input frame (grayscale conversion and rescaling)
        rayon::spawn(move || {
            let mut frame_count = 1;
            loop {
                if let Ok(events) = events_rx.recv() {
                    // assert_eq!(events.len(), (self.source.get_video().height as f64 / self.framer.chunk_rows as f64).ceil() as usize);

                    // Frame the events
                    if framer.ingest_events_events(events) {
                        match framer.write_multi_frame_bytes(&mut output_stream) {
                            Ok(0) => {
                                eprintln!("Should have frame, but didn't");
                                break;
                            }
                            Ok(frames_returned) => {
                                frame_count += frames_returned;
                                print!(
                                    "\rOutput frame {}. Got {} frames in  {} ms/frame\t",
                                    frame_count,
                                    frames_returned,
                                    now.elapsed().as_millis() / frames_returned as u128
                                );
                                if io::stdout().flush().is_err() {
                                    eprintln!("Error flushing stdout");
                                    break;
                                };
                                now = Instant::now();
                            }
                            Err(e) => {
                                eprintln!("Error writing frame: {e}");
                                break;
                            }
                        }
                    }
                    if output_stream.flush().is_err() {
                        eprintln!("Error flushing output stream");
                        break;
                    }
                    if frame_count >= frame_max && frame_max > 0 {
                        eprintln!("Wrote max frames. Exiting channel.");
                        break;
                    }
                } else {
                    eprintln!("Event receiver is closed. Exiting channel.");
                    break;
                };
            }
        });

        Ok(SimulProcessor {
            source,
            thread_pool: thread_pool_transcoder,
            events_tx,
        })
    }

    /// Run the processor
    /// This will run until the source is exhausted
    pub fn run(&mut self) -> Result<(), Box<dyn Error>> {
        let mut now = Instant::now();

        loop {
            match self.source.consume(1, &self.thread_pool) {
                Ok(_events) => {
                    // match self.events_tx.send(events) {
                    //     Ok(_) => {}
                    //     Err(_) => {
                    //         break;
                    //     }
                    // };
                }
                Err(e) => {
                    println!("Err: {e:?}");
                    break;
                }
            };

            let video = self.source.get_video_ref();

            if video.state.in_interval_count % 30 == 0 {
                print!(
                    "\rFrame {} in  {}ms",
                    video.state.in_interval_count,
                    now.elapsed().as_millis()
                );
                if io::stdout().flush().is_err() {
                    eprintln!("Error flushing stdout");
                    break;
                };
                now = Instant::now();
            }
            // // TODO: temp
            // if video.state.in_interval_count == 30 {
            //     break;
            // }
        }

        println!("Closing stream...");
        self.source.get_video_mut().end_write_stream()?;
        println!("FINISHED");

        Ok(())
    }
}
