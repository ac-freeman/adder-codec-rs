use adder_codec_rs::framer::scale_intensity::event_to_intensity;
use adder_codec_rs::raw::stream::Raw;
use adder_codec_rs::utils::stream_migration::absolute_event_to_dt_event;
use adder_codec_rs::TimeMode::AbsoluteT;
use adder_codec_rs::{Codec, DeltaT, Intensity, D_SHIFT};
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
    let file_path = args.input.as_str();

    let mut stream: Raw = Codec::new();
    stream.open_reader(file_path).expect("Invalid path");
    let header_bytes = stream.decode_header().expect("Invalid header");
    let first_event_position = stream.get_input_stream_position()?;

    let eof_position_bytes = stream.get_eof_position()?;
    let file_size = Path::new(file_path).metadata()?.len();
    let num_events = (eof_position_bytes - 1 - header_bytes as u64) / stream.event_size as u64;
    let events_per_px = num_events / stream.plane.volume() as u64;

    let stdout = io::stdout();
    let mut handle = io::BufWriter::new(stdout.lock());

    writeln!(handle, "Dimensions")?;
    writeln!(handle, "\tWidth: {}", stream.plane.w())?;
    writeln!(handle, "\tHeight: {}", stream.plane.h())?;
    writeln!(handle, "\tColor channels: {}", stream.plane.c())?;
    writeln!(handle, "Source camera: {}", stream.source_camera)?;
    writeln!(handle, "ADΔER transcoder parameters")?;
    writeln!(handle, "\tCodec version: {}", stream.codec_version)?;
    writeln!(handle, "\tTime mode: {}", stream.time_mode)?;
    writeln!(handle, "\tTicks per second: {}", stream.tps)?;
    writeln!(
        handle,
        "\tReference ticks per source interval: {}",
        stream.ref_interval
    )?;
    writeln!(handle, "\tΔt_max: {}", stream.delta_t_max)?;
    writeln!(handle, "File metadata")?;
    writeln!(handle, "\tFile size: {}", file_size)?;
    writeln!(handle, "\tHeader size: {}", header_bytes)?;
    writeln!(handle, "\tADΔER event count: {}", num_events)?;
    writeln!(handle, "\tEvents per pixel channel: {}", events_per_px)?;
    handle.flush()?;

    // Calculate the dynamic range of the events. That is, what is the highest intensity
    // event, and what is the lowest intensity event?
    if args.dynamic_range {
        let divisor = num_events / 100;
        stream.set_input_stream_position(first_event_position)?;
        let mut max_intensity: Intensity = 0.0;
        let mut min_intensity: Intensity = f64::MAX;
        let mut event_count: u64 = 0;

        // Setup time tracker for AbsoluteT mode
        let mut data = Vec::new();
        for _ in 0..stream.plane.volume() {
            let t = 0_u32;
            data.push(t);
        }
        let mut t_tree: Array3<DeltaT> = Array3::from_shape_vec(
            (
                stream.plane.h_usize(),
                stream.plane.w_usize(),
                stream.plane.c_usize(),
            ),
            data,
        )?;

        while let Ok(mut event) = stream.decode_event() {
            if stream.codec_version >= 2 && stream.time_mode == AbsoluteT {
                let last_t = &mut t_tree[[
                    event.coord.y_usize(),
                    event.coord.x_usize(),
                    event.coord.c_usize(),
                ]];
                let new_t = event.delta_t;
                event = absolute_event_to_dt_event(event, *last_t);
                *last_t = new_t;
            }

            match event_to_intensity(&event) {
                _ if event.d == 0xFF => {
                    // ignore empty events
                }
                a if a.is_infinite() => {
                    println!("INFINITE");
                    dbg!(event);
                }
                a if a < min_intensity => {
                    if event.d == 0xFE {
                        min_intensity = 1.0 / event.delta_t as f64;
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

        let theory_dr_ratio = D_SHIFT[D_SHIFT.len() - 1] as f64 / (1.0 / stream.delta_t_max as f64);
        let theory_dr_db = 10.0 * theory_dr_ratio.log10();
        let theory_dr_bits = theory_dr_ratio.log2();
        writeln!(handle, "\rDynamic range                       ")?;
        writeln!(handle, "\tTheoretical range:")?;
        writeln!(handle, "\t\t{:.4} dB (power)", theory_dr_db)?;
        writeln!(handle, "\t\t{:.4} bits", theory_dr_bits)?;

        let real_dr_ratio = max_intensity / min_intensity;
        let real_dr_db = 10.0 * real_dr_ratio.log10();
        let real_dr_bits = real_dr_ratio.log2();
        writeln!(handle, "\tRealized range:")?;
        writeln!(handle, "\t\t{:.4} dB (power)", real_dr_db)?;
        writeln!(handle, "\t\t{:.4} bits", real_dr_bits)?;
    }

    handle.flush()?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use assert_cmd::prelude::*;
    use predicates::prelude::*;
    use std::process::Command;

    #[test]
    fn test_adder_info() -> Result<(), Box<dyn std::error::Error>> {
        let mut cmd = Command::cargo_bin("adder-info")?;

        cmd.arg("--input").arg("tests/test_sample.adder");
        cmd.arg("-d");
        cmd.assert()
            .success()
            .stdout(predicate::str::contains("Width: 2"))
            .stdout(predicate::str::contains("Height: 2"))
            .stdout(predicate::str::contains("Color channels: 1"))
            .stdout(predicate::str::contains("Source camera: FramedU8"))
            .stdout(predicate::str::contains("Codec version: 1"))
            .stdout(predicate::str::contains("Ticks per second: 120000"))
            .stdout(predicate::str::contains("ticks per source interval: 5000"))
            .stdout(predicate::str::contains("t_max: 240000"))
            .stdout(predicate::str::contains("File size: 1307"))
            .stdout(predicate::str::contains("Header size: 29"))
            .stdout(predicate::str::contains("event count: 140"))
            .stdout(predicate::str::contains("Events per pixel channel: 35"));

        Ok(())
    }
}
