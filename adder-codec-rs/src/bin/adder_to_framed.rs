use adder_codec_core::open_file_decoder;
use adder_codec_core::SourceType::U8;
use adder_codec_rs::framer::driver::FramerMode::INSTANTANEOUS;
use adder_codec_rs::framer::driver::{FrameSequence, Framer, FramerBuilder};
use clap::Parser;
use std::fs::File;
use std::io;
use std::io::{BufWriter, Write};
use std::process::Command;
use std::time::Instant;

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

    /// Number of threads to use. If not provided, will default to the number of cores on the
    /// system.
    #[clap(long, default_value_t = 8)]
    pub thread_count: u8,

    #[clap(short, long, action)]
    pub features: bool,

    /// Frames per second to derive the video from the adder events
    #[clap(short, long, default_value_t = 30.0)]
    pub fps: f64,

    /// For encoding with ffmpeg, the playback FPS = fps above * playback_speed
    /// This is useful for slow motion or fast forward
    #[clap(short, long, default_value_t = 1.0)]
    pub playback_speed: f64,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: MyArgs = MyArgs::parse();

    // Set up the adder file reader
    let (mut reader, mut bitreader) = open_file_decoder(&args.input)?;

    let meta = *reader.meta();

    let mut framer: FrameSequence<u8> = FramerBuilder::new(meta.plane, 1)
        .codec_version(meta.codec_version, meta.time_mode)
        .time_parameters(
            meta.tps,
            meta.ref_interval,
            meta.delta_t_max,
            Some(args.fps as f32),
        )
        .mode(INSTANTANEOUS)
        .source(U8, meta.source_camera)
        .finish::<u8>();

    let mut output_stream = BufWriter::new(File::create(&args.output)?);
    let mut frame_count = 0;
    let mut now = Instant::now();
    //
    loop {
        let res = reader.digest_event(&mut bitreader);
        // read an event
        if let Ok(mut event) = res {
            // if now.elapsed().as_millis() > 100 {
            //     // this is a hacky way of limiting the buffer size
            //     eprintln!("Flushing");
            //     framer.flush_frame_buffer();
            // }
            // ingest the event
            if framer.ingest_event(&mut event, None) {
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
        } else {
            dbg!(res);
            break;
        }
    }
    while framer.flush_frame_buffer() {
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
    dbg!(frame_count);

    // Use ffmpeg to encode the raw frame data as an mp4
    let color_str = if meta.plane.c() != 1 { "rgb24" } else { "gray" };

    let mut ffmpeg = Command::new("sh")
        .arg("-c")
        .arg(
            "ffmpeg -hide_banner -loglevel error -f rawvideo -pix_fmt ".to_owned()
                + color_str
                + " -s:v "
                + meta.plane.w().to_string().as_str()
                + "x"
                + meta.plane.h().to_string().as_str()
                + " -r "
                + (args.fps * args.playback_speed).to_string().as_str()
                + " -i "
                + &args.output
                + " -crf 0 -c:v libx264 -y "
                + &args.output
                + ".mp4",
        )
        .spawn()?;
    ffmpeg.wait()?;

    Ok(())
}
