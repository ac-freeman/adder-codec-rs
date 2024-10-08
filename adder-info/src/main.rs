use crate::TimeMode::AbsoluteT;
use adder_codec_core::*;
use adder_codec_rs::framer::scale_intensity::event_to_intensity;
use adder_codec_rs::utils::stream_migration::absolute_event_to_dt_event;
use clap::Parser;
use ndarray::Array3;
use std::io::Write;
use std::path::Path;
use std::{error, io};

/// Command line argument parser
#[derive(Parser, Debug, Default)]
#[clap(author, version, about, long_about = None)]
pub struct MyArgs {
    /// Input ADΔER video path
    #[clap(short, long)]
    pub(crate) input: String,

    /// Calculate dynamic range of the event stream? (Takes more time)
    #[clap(short, long, action)]
    pub(crate) dynamic_range: bool,
}

fn main() -> Result<(), Box<dyn error::Error>> {
    let args: MyArgs = MyArgs::parse();
    adder_info(args, io::stdout())?;
    Ok(())
}

fn adder_info(args: MyArgs, out: impl Write) -> Result<(), Box<dyn error::Error>> {
    let file_path = args.input.as_str();
    let (mut stream, mut bitreader) = open_file_decoder(file_path)?;

    let first_event_position = stream.get_input_stream_position(&mut bitreader)?;

    let eof_position_bytes = stream.get_eof_position(&mut bitreader)?;
    let file_size = Path::new(file_path).metadata()?.len();

    let meta = *stream.meta();

    // TODO: Need a different mechanism for compressed files
    let num_events = (eof_position_bytes - 1 - meta.header_size as u64) / meta.event_size as u64;
    let events_per_px = num_events / meta.plane.volume() as u64;

    let mut handle = io::BufWriter::new(out);

    writeln!(handle, "Dimensions")?;
    writeln!(handle, "\tWidth: {}", meta.plane.w())?;
    writeln!(handle, "\tHeight: {}", meta.plane.h())?;
    writeln!(handle, "\tColor channels: {}", meta.plane.c())?;
    writeln!(handle, "Source camera: {:?}", meta.source_camera)?;
    writeln!(handle, "ADΔER transcoder parameters")?;
    writeln!(handle, "\tCodec version: {}", meta.codec_version)?;
    writeln!(handle, "\tTime mode: {:?}", meta.time_mode)?;
    writeln!(handle, "\tTicks per second: {}", meta.tps)?;
    writeln!(
        handle,
        "\tReference ticks per source interval: {}",
        meta.ref_interval
    )?;
    writeln!(handle, "\tΔt_max: {}", meta.delta_t_max)?;
    writeln!(handle, "File metadata")?;
    writeln!(handle, "\tFile size: {file_size}")?;
    writeln!(handle, "\tHeader size: {0}", meta.header_size)?;
    writeln!(handle, "\tADΔER event count: {num_events}")?;
    writeln!(handle, "\tEvents per pixel channel: {events_per_px}")?;
    handle.flush()?;

    // Calculate the dynamic range of the events. That is, what is the highest intensity
    // event, and what is the lowest intensity event?
    if args.dynamic_range {
        let divisor = num_events / 100;
        stream.set_input_stream_position(&mut bitreader, first_event_position)?;
        let mut max_intensity: Intensity = 0.0;
        let mut min_intensity: Intensity = f64::MAX;
        let mut event_count: u64 = 0;

        // Setup time tracker for AbsoluteT mode
        let data = vec![0_u32; meta.plane.volume()];

        let mut t_tree: Array3<DeltaT> = Array3::from_shape_vec(
            (
                meta.plane.h_usize(),
                meta.plane.w_usize(),
                meta.plane.c_usize(),
            ),
            data,
        )?;

        while let Ok(mut event) = stream.digest_event(&mut bitreader) {
            if meta.codec_version >= 2 && meta.time_mode == AbsoluteT {
                let last_t = &mut t_tree[[
                    event.coord.y_usize(),
                    event.coord.x_usize(),
                    event.coord.c_usize(),
                ]];
                let new_t = event.t;
                event = absolute_event_to_dt_event(event, *last_t);
                *last_t = new_t;
            }

            match event_to_intensity(&event) {
                _ if event.d == D_EMPTY => {
                    // ignore empty events
                }
                a if a.is_infinite() => {
                    println!("INFINITE");
                    dbg!(event);
                }
                a if a < min_intensity => {
                    if event.d == D_ZERO_INTEGRATION {
                        min_intensity = 1.0 / event.t as f64;
                    } else {
                        min_intensity = a;
                    }
                }
                a if a > max_intensity => {
                    max_intensity = a;
                }
                _ => {}
            }

            event_count += 1;
            if event_count % divisor == 0 {
                write!(
                    handle,
                    "\rCalculating dynamic range...{}%",
                    (event_count * 100) / num_events
                )?;
                handle.flush()?;
            }
        }

        let theory_dr_ratio = D_SHIFT[D_SHIFT.len() - 1] as f64 / (1.0 / meta.delta_t_max as f64);
        let theory_dr_db = 10.0 * theory_dr_ratio.log10();
        let theory_dr_bits = theory_dr_ratio.log2();
        writeln!(handle, "\rDynamic range                       ")?;
        writeln!(handle, "\tTheoretical range:")?;
        writeln!(handle, "\t\t{theory_dr_db:.4} dB (power)")?;
        writeln!(handle, "\t\t{theory_dr_bits:.4} bits")?;

        let real_dr_ratio = max_intensity / min_intensity;
        let real_dr_db = 10.0 * real_dr_ratio.log10();
        let real_dr_bits = real_dr_ratio.log2();
        writeln!(handle, "\tRealized range:")?;
        writeln!(handle, "\t\t{real_dr_db:.4} dB (power)")?;
        writeln!(handle, "\t\t{real_dr_bits:.4} bits")?;
    }

    handle.flush()?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::{adder_info, MyArgs};
    use std::io::Cursor;

    #[test]
    fn test_adder_info() -> Result<(), Box<dyn std::error::Error>> {
        let args = MyArgs {
            input: "./tests/test_sample.adder".to_string(),
            dynamic_range: true,
        };

        let mut data = Vec::new();
        {
            let cursor = Cursor::new(&mut data);

            adder_info(args, cursor)?;
        }

        let string = String::from_utf8(data)?;

        assert!(string.contains("Width: 2"));
        assert!(string.contains("Height: 2"));
        assert!(string.contains("Color channels: 1"));
        assert!(string.contains("Source camera: FramedU8"));
        assert!(string.contains("Codec version: 1"));
        assert!(string.contains("Ticks per second: 120000"));
        assert!(string.contains("ticks per source interval: 5000"));
        assert!(string.contains("t_max: 240000"));
        assert!(string.contains("File size: 1307"));
        assert!(string.contains("Header size: 29"));
        assert!(string.contains("event count: 137"));
        assert!(string.contains("Events per pixel channel: 34"));

        Ok(())
    }
}
