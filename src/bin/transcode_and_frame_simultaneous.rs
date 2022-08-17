extern crate core;

use adder_codec_rs::framer::event_framer::FramerMode::INSTANTANEOUS;
use adder_codec_rs::framer::event_framer::SourceType::U8;
use adder_codec_rs::framer::event_framer::{FrameSequence, Framer, SourceType};
use adder_codec_rs::framer::scale_intensity;
use adder_codec_rs::framer::scale_intensity::FrameValue;
use adder_codec_rs::transcoder::source::framed_source::FramedSource;
use adder_codec_rs::transcoder::source::video::Source;
use adder_codec_rs::SourceCamera::FramedU8;
use adder_codec_rs::{DeltaT, Event, SourceCamera};
use rayon::{current_num_threads, ThreadPool};
use reqwest;
use serde::Serialize;
use std::error::Error;
use std::fs::File;
use std::io;
use std::io::{BufWriter, Cursor, Write};
use std::path::Path;
use std::process::Command;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::Instant;

async fn download_file() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Download the drop.mp4 video example, if you don't already have it
    let path_str = "./tests/samples/videos/drop.mp4";
    if !Path::new(path_str).exists() {
        let mut resp = reqwest::get("https://www.pexels.com/video/2603664/download/").await?;
        let mut file_out = File::create(path_str).expect("Could not create file on disk");
        let mut data_in = Cursor::new(resp.bytes().await?);
        std::io::copy(&mut data_in, &mut file_out)?;
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    download_file().await.unwrap();

    let ref_time = 5000;
    let tps = 120000;
    let delta_t_max = 120000;

    let mut source = FramedSource::new(
        "./tests/samples/videos/drop.mp4".to_string(),
        0,
        ref_time,
        tps,
        delta_t_max,
        0.5,
        0,
        false,
        true,
        10,
        5,
        true,
        true,
        true,
        SourceCamera::FramedU8,
    )
    .unwrap();

    let output_path = "./tests/samples/videos/drop_adder.gray8";
    let mut simul_processor = SimulProcessor::new::<u8>(source, ref_time, tps, output_path);

    let now = std::time::Instant::now();
    simul_processor.run(200).unwrap();

    // Use ffmpeg to encode the raw frame data as an mp4
    Command::new("sh")
        .arg("-c")
        .arg(
            "ffmpeg -f rawvideo -pix_fmt gray -s:v 1920x1080 -r 24 -i ".to_owned()
                + &output_path.to_owned()
                + " -crf 0 -c:v libx264 -y drop_recon.mp4",
        )
        .spawn()
        .unwrap();
    println!("{} ms elapsed", now.elapsed().as_millis());

    Ok(())
}

pub(crate) struct SimulProcessor {
    source: FramedSource,
    thread_pool: ThreadPool,
    events_tx: Sender<Vec<Vec<Event>>>,
}

impl SimulProcessor {
    pub fn new<T>(
        mut source: FramedSource,
        ref_time: DeltaT,
        tps: DeltaT,
        output_path: &str,
    ) -> SimulProcessor
    where
        T: Clone + std::marker::Sync + std::marker::Send + 'static,
        T: scale_intensity::FrameValue,
        T: std::default::Default,
        T: std::marker::Copy,
        T: FrameValue<Output = T>,
        T: Serialize,
    {
        let thread_pool = rayon::ThreadPoolBuilder::new()
            // .num_threads(1)
            .num_threads(current_num_threads() / 2)
            .build()
            .unwrap();
        let reconstructed_frame_rate = 24;
        // For instantaneous reconstruction, make sure the frame rate matches the source video rate
        assert_eq!(tps / ref_time, reconstructed_frame_rate);

        let height = source.get_video().height as usize;
        let width = source.get_video().width as usize;
        let channels = source.get_video().channels as usize;

        let mut framer = thread_pool.install(|| {
            FrameSequence::<T>::new(
                height,
                width,
                channels,
                tps,
                reconstructed_frame_rate,
                INSTANTANEOUS,
                U8,
                1,
                FramedU8,
                ref_time,
            )
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
                                        "\rOutput frame {}. Got {} frames in  {}ms",
                                        frame_count,
                                        frames_returned,
                                        now.elapsed().as_millis()
                                    );
                                    io::stdout().flush().unwrap();
                                    now = Instant::now();
                                }
                            }
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
            thread_pool,
            events_tx,
        }
    }

    pub fn run(&mut self, frame_max: u32) -> Result<(), Box<dyn Error>> {
        let mut now = Instant::now();

        loop {
            match self.thread_pool.install(|| self.source.consume(1)) {
                Ok(events) => {
                    // self.framify_new_events(events, output_1.0)
                    self.events_tx.send(events);
                }
                Err("End of video") => break, // TODO: make it a proper rust error
                Err(_) => {}
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
            if frame_max != 0 && video.in_interval_count >= frame_max {
                break;
            }
        }

        println!("Closing stream...");
        self.source.get_video_mut().end_write_stream();
        println!("FINISHED");

        Ok(())
    }
}
