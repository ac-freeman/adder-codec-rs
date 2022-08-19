use adder_codec_rs::raw::raw_stream::RawStream;
use adder_codec_rs::Codec;
use std::io::Write;
use std::path::Path;
use std::{env, io};

fn main() -> Result<(), std::io::Error> {
    let args: Vec<String> = env::args().collect();
    let file_path = &args[1];

    let mut stream: RawStream = Codec::new();
    stream.open_reader(file_path).expect("Invalid path");
    let header_bytes = stream.decode_header().expect("Invalid header");
    let eof_position_bytes = stream.get_eof_position().unwrap();
    let file_size = Path::new(file_path).metadata().unwrap().len();
    let num_events = (eof_position_bytes - 1 - header_bytes) / stream.event_size as usize;

    let stdout = io::stdout();
    let mut handle = io::BufWriter::new(stdout.lock());
    writeln!(handle, "Dimensions")?;
    writeln!(handle, "\tWidth: {}", stream.width)?;
    writeln!(handle, "\tHeight: {}", stream.height)?;
    writeln!(handle, "\tColor channels: {}", stream.channels)?;
    writeln!(handle, "Source camera: {}", stream.source_camera)?;
    writeln!(handle, "ADΔER transcoder parameters")?;
    writeln!(handle, "\tCodec version: {}", stream.codec_version)?;
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

    handle.flush().unwrap();
    Ok(())
}
