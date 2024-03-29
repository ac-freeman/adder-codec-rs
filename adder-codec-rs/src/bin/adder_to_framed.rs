use adder_codec_core::codec::compressed::stream::CompressedInput;
use adder_codec_core::codec::decoder::Decoder;
use adder_codec_core::codec::rate_controller::Crf;
use adder_codec_core::codec::raw::stream::RawInput;
use adder_codec_core::codec::{EncoderOptions, EncoderType};
use adder_codec_core::SourceCamera::{Dvs, FramedU8};
use adder_codec_core::SourceType::U8;
use adder_codec_core::{open_file_decoder, PixelMultiMode, TimeMode};
use adder_codec_rs::framer::driver::FramerMode::INSTANTANEOUS;
use adder_codec_rs::framer::driver::{FrameSequence, Framer, FramerBuilder};
use adder_codec_rs::transcoder::source::prophesee::Prophesee;
use adder_codec_rs::transcoder::source::video::SourceError;
use adder_codec_rs::utils::viz::ShowFeatureMode;
use bitstream_io::{BigEndian, BitReader};
use clap::Parser;
use std::fs::File;
use std::io;
use std::io::{BufReader, BufWriter, Write};
use std::process::Command;
use std::time::Instant;
use video_rs_adder_dep::Locator;

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

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args: MyArgs = MyArgs::parse();

    // Set up the adder file reader
    let (mut reader, mut bitreader) = open_file_decoder(&args.input)?;

    let meta = reader.meta().clone();

    let mut framer: FrameSequence<u8> = FramerBuilder::new(meta.plane, 1)
        .codec_version(meta.codec_version, meta.time_mode)
        .time_parameters(meta.tps, meta.ref_interval, meta.delta_t_max, Some(30.0))
        .mode(INSTANTANEOUS)
        .source(U8, FramedU8)
        .finish::<u8>();

    let mut output_stream = BufWriter::new(File::create(&args.output)?);
    let mut frame_count = 0;
    let mut now = Instant::now();
    //
    loop {
        // read an event
        if let Ok(mut event) = reader.digest_event(&mut bitreader) {
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
    let color_str = match meta.plane.c() != 1 {
        true => "bgr24",
        _ => "gray",
    };

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
                + "30.0"
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
