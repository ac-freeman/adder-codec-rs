use adder_codec_rs::codec::raw::stream::Raw;
use adder_codec_rs::codec::Codec;
use adder_codec_rs::framer::scale_intensity::event_to_intensity;
use adder_codec_rs::transcoder::source::video::show_display_force;
use adder_codec_rs::{SourceCamera, D_ZERO_INTEGRATION};
use clap::Parser;
use opencv::core::{create_continuous, Mat, MatTraitManual, CV_64F, CV_64FC3};
use std::cmp::max;
use std::error::Error;
use std::fs::File;
use std::io::Write;
use std::{fmt, io};
use tokio::time::Instant;

/// Command line argument parser
#[derive(Parser, Debug, Default)]
#[clap(author, version, about, long_about = None)]
pub struct MyArgs {
    /// Input aedat4 file path
    #[clap(short, long)]
    pub(crate) input: String,

    /// Target playback frame rate. Might not actually meet this rate, or keep it consistently,
    /// depending on the rate of decoding ADΔER events.
    #[clap(short = 'f', long, default_value_t = 60.0)]
    pub playback_fps: f64,
}

#[derive(Debug)]
struct PlayerError(String);

impl fmt::Display for PlayerError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "There is an error: {}", self.0)
    }
}

impl Error for PlayerError {}

///
/// This program visualizes the ADΔER events, akin to a traditional video player. It simply
/// displays the intensity of the most recently fired event for each pixel.
/// There will likely be artifacts present where the events aren't perfectly temporally interleaved,
/// but these artifacts are not present when performing a full framed reconstruction with
/// `events_to_instantaneous_frames.rs`. Future work will involve re-ordering the ADΔER events
/// to be temporally interleaved, to mitigate these real-time playback artifacts.
///
#[allow(dead_code)]
fn main() -> Result<(), Box<dyn Error>> {
    let args: MyArgs = MyArgs::parse();
    let file_path = args.input.as_str();

    let mut stream: Raw = Codec::new();
    let file = File::open(file_path)?;
    stream.set_input_stream(Some(io::BufReader::new(file)));
    let header_bytes = stream.decode_header().expect("Invalid header");

    let first_event_position = stream.get_input_stream_position()?;

    let eof_position_bytes = stream.get_eof_position()?;
    let num_events = (eof_position_bytes - 1 - header_bytes as u64) / u64::from(stream.event_size);
    let divisor = num_events / 100;
    let frame_length = f64::from(stream.tps) / args.playback_fps;

    let stdout = io::stdout();
    let mut handle = io::BufWriter::new(stdout.lock());

    stream.set_input_stream_position(first_event_position)?;

    let mut display_mat = Mat::default();
    match stream.plane.c() {
        1 => {
            create_continuous(
                i32::from(stream.plane.h()),
                i32::from(stream.plane.w()),
                CV_64F,
                &mut display_mat,
            )?;
        }
        3 => {
            create_continuous(
                i32::from(stream.plane.h()),
                i32::from(stream.plane.w()),
                CV_64FC3,
                &mut display_mat,
            )?;
        }
        _ => {
            return Err(Box::new(PlayerError("Bad number of channels".into())));
        }
    }

    let mut event_count: u64 = 0;
    let mut current_t = 0;
    let mut frame_count: u128 = 1;
    let mut last_frame_displayed_ts = Instant::now();
    loop {
        if event_count % divisor == 0 {
            write!(
                handle,
                "\rPlaying back ADΔER file...{}%",
                (event_count * 100) / num_events
            )?;
            handle.flush()?;
        }
        if u128::from(current_t) > (frame_count * frame_length as u128) {
            let wait_time = max(
                ((1000.0 / args.playback_fps) as u128)
                    .saturating_sub((Instant::now() - last_frame_displayed_ts).as_millis()),
                1,
            ) as i32;
            show_display_force("ADΔER", &display_mat, wait_time)?;
            last_frame_displayed_ts = Instant::now();
            frame_count += 1;
        }

        match stream.decode_event() {
            Ok(event) if event.d <= D_ZERO_INTEGRATION => {
                event_count += 1;
                let y = i32::from(event.coord.y);
                let x = i32::from(event.coord.x);
                let c = i32::from(event.coord.c.unwrap_or(0));
                if (y | x | c) == 0x0 {
                    current_t += event.delta_t;
                }

                let frame_intensity = (event_to_intensity(&event) * f64::from(stream.ref_interval))
                    / match stream.source_camera {
                        SourceCamera::FramedU8 => f64::from(u8::MAX),
                        SourceCamera::FramedU16 => f64::from(u16::MAX),
                        SourceCamera::FramedU32 => f64::from(u32::MAX),
                        SourceCamera::FramedU64 => u64::MAX as f64,
                        SourceCamera::FramedF32 => {
                            todo!("Not yet implemented")
                        }
                        SourceCamera::FramedF64 => {
                            todo!("Not yet implemented")
                        }
                        SourceCamera::Dvs => f64::from(u8::MAX),
                        SourceCamera::DavisU8 => f64::from(u8::MAX),
                        SourceCamera::Atis => {
                            todo!("Not yet implemented")
                        }
                        SourceCamera::Asint => {
                            todo!("Not yet implemented")
                        }
                    };
                unsafe {
                    let px: &mut f64 = display_mat.at_3d_unchecked_mut(y, x, c)?;
                    *px = frame_intensity;
                }
            }
            Err(_e) => {
                break;
            }
            _ => {}
        }
    }

    Ok(())
}
