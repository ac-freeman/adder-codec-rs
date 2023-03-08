use adder_codec_rs::transcoder::source::davis::get_next_image;
use adder_codec_rs::transcoder::source::video::show_display_force;
use adder_codec_rs::utils::viz::{encode_video_ffmpeg, write_frame_to_video};
use aedat::events_generated::Event;
use clap::Parser;
use davis_edi_rs::util::reconstructor::Reconstructor;
use opencv::core::{Mat, MatTrait, MatTraitManual, CV_8U};
use rayon::current_num_threads;
use std::cmp::max;
use std::collections::VecDeque;
use std::error;
use std::fs::File;
use std::io::BufWriter;

/// Command line argument parser
#[derive(Parser, Debug, Default)]
#[clap(author, version, about, long_about = None)]
pub struct MyArgs {
    /// Input aedat4 file path
    #[clap(short, long)]
    pub(crate) input: String,

    /// Output DVS event video file path
    #[clap(long)]
    pub(crate) output_video: String,

    /// Show display?
    #[clap(long, action)]
    pub show_display: bool,

    #[clap(long, default_value_t = 100.0)]
    pub fps: f32,
}

///
/// This program visualizes the DVS events within an AEDAT4 file. The visualization is written
/// out as an mp4 file. For simplicity, I piggyback off my EDI implementation, although this
/// adds some overhead.
///
#[allow(dead_code)]
fn main() -> Result<(), Box<dyn error::Error>> {
    let args: MyArgs = MyArgs::parse();
    let file_path = args.input.as_str();

    let output_video_path = args.output_video.as_str();
    let raw_path = "./dvs.gray8";

    let aedat_filename = file_path.split('/').last().expect("Invalid file path");
    let base_path = file_path
        .split(aedat_filename)
        .next()
        .expect("Invalid file path");
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(12)
        .build()?;
    let thread_pool_edi = rayon::ThreadPoolBuilder::new()
        .num_threads(max(current_num_threads() - 4, 1))
        .build()?;
    let mut reconstructor = rt.block_on(Reconstructor::new(
        base_path.to_string(),
        aedat_filename.to_string(),
        String::new(),
        "file".to_string(),
        0.15,
        false,
        1,
        false,
        false,
        false,
        60.0,
        true,
        true,
        1.0,
        false,
    ));

    let mut instantaneous_frame_deque = unsafe {
        let mut instantaneous_frame = Mat::default();
        instantaneous_frame.create_rows_cols(260, 346, CV_8U)?;

        VecDeque::from([instantaneous_frame])
    };
    match instantaneous_frame_deque
        .back_mut()
        .expect("Could not get back of deque")
        .data_bytes_mut()
    {
        Ok(bytes) => {
            for byte in bytes {
                *byte = 128;
            }
        }
        Err(e) => {
            return Err(Box::new(e));
        }
    }

    let mut video_writer: BufWriter<File> = BufWriter::new(File::create(raw_path)?);

    let frame_length = (1_000_000.0 / args.fps) as u128; // length in ticks
    let mut frame_count = 0_usize;
    let mut base_t = 0;
    let mut current_t = 0;
    let mut event_count: u128 = 0;
    //TODO: Restore this

    // let mut init = None;

    // loop {
    //     let mat_opt = rt.block_on(get_next_image(&mut reconstructor, &thread_pool_edi, true));
    //
    //     match mat_opt {
    //         Ok(None) => {
    //             break;
    //         }
    //         Ok(Some((_, _, Some((_, _, events, _, _))))) => match init {
    //             None => init = Some(()),
    //             Some(()) => {
    //                 for event in events {
    //                     event_count += 1;
    //                     if current_t > (frame_count as u128 * frame_length) + 1_000_000 {
    //                         match instantaneous_frame_deque.pop_front() {
    //                             None => {}
    //                             Some(frame) => {
    //                                 if args.show_display {
    //                                     show_display_force("DVS", &frame, 1)?;
    //                                 }
    //                                 write_frame_to_video(&frame, &mut video_writer)?;
    //                             }
    //                         }
    //                         frame_count += 1;
    //                     }
    //                     if base_t == 0 {
    //                         base_t = event.t() as u128;
    //                     }
    //
    //                     current_t = max(event.t() as u128 - base_t, current_t);
    //                     let frame_idx = ((event.t() as u128 - base_t) / frame_length) as usize;
    //
    //                     set_instant_dvs_pixel(
    //                         event,
    //                         &mut instantaneous_frame_deque,
    //                         frame_idx,
    //                         frame_count,
    //                     )?;
    //                 }
    //             }
    //         },
    //         Ok(Some((_, _, None))) => {
    //             break;
    //         }
    //         Err(e) => {
    //             println!("Error: {e:?}");
    //             break;
    //         }
    //     }
    // }

    for frame in instantaneous_frame_deque {
        if args.show_display {
            show_display_force("DVS", &frame, 1)?;
        }
        write_frame_to_video(&frame, &mut video_writer)?;
    }
    println!("\nDVS event count: {event_count}");
    println!("\n");
    encode_video_ffmpeg(raw_path, output_video_path)?;
    println!("Finished!");

    Ok(())
}

fn set_instant_dvs_pixel(
    event: Event,
    frames: &mut VecDeque<Mat>,
    frame_idx: usize,
    frame_count: usize,
) -> opencv::Result<()> {
    // Grow the deque if necessary
    let grow_len = frame_idx as i32 - frame_count as i32 - frames.len() as i32 + 1;
    for _ in 0..grow_len {
        frames.push_back(frames[0].clone());
        // Clear the instantaneous frame
        match frames
            .back_mut()
            .expect("Can't get back of deque")
            .data_bytes_mut()
        {
            Ok(bytes) => {
                for byte in bytes {
                    *byte = 128;
                }
            }
            Err(e) => {
                return Err(e);
            }
        }
    }

    unsafe {
        let px: &mut u8 = frames[frame_idx - frame_count]
            .at_2d_unchecked_mut(event.y().into(), event.x().into())?;
        *px = match event.on() {
            true => 255,
            false => 0,
        }
        // match value {
        //     128 => *px = 128,
        //     a => *px = (*px as i16 + a) as u8,
        // }
    }
    Ok(())
}
