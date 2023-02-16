use crate::framer::driver::SourceType;
use crate::{Coord, DeltaT, Event, EOF_PX_ADDRESS};
use bitstream_io::{BigEndian, BitRead, BitWrite, BitWriter, Numeric, SignedNumeric};
use std::fmt::Arguments;
use std::fs::{File, Metadata};
use std::io::{BufReader, BufWriter, IoSlice, Read, Seek, Write};
use std::path::Path;
use std::{fmt, io, mem};

pub mod compressed;
mod units;
mod utils;

// pub trait Codec {
//     fn get_source_type(&self) -> SourceType;
//     fn new() -> Self
//     where
//         Self: Sized;
// }
//
// pub trait Encoder<W: Write + Seek>: Codec {
//     /// Create a file writer at the given `path`
//     /// # Arguments
//     /// * `path` - The path to the file to write to
//     /// # Errors
//     /// * If the file cannot be created
//     // fn open_writer<P: AsRef<Path>>(&mut self, path: P) -> Result<(), std::io::Error> {
//     //     let file = File::create(&path)?;
//     //     self.set_output_stream(Some(BufWriter::new(file)));
//     //     Ok(())
//     // }
//     //
//     // /// Set the input stream to read from
//     // /// # Errors
//     // /// * If the input stream cannot be opened
//     // fn open_reader<P: AsRef<Path>>(&mut self, path: P) -> Result<(), std::io::Error> {
//     //     let file = File::open(&path)?;
//     //     self.set_input_stream(Some(BufReader::new(file)));
//     //     Ok(())
//     // }
//
//     fn set_output_stream(&mut self, stream: Option<W>);
//
//     /// Write the EOF event signifier to the output stream
//     /// # Errors
//     /// * If the EOF event cannot be written
//     fn write_eof(&mut self) -> Result<(), StreamError>;
//
//     /// Flush the stream so that program can be exited safely
//     /// # Errors
//     /// * If the stream cannot be flushed
//     fn flush_writer(&mut self) -> io::Result<()>;
//
//     /// Close the stream writer safely
//     /// # Errors
//     /// * If the stream cannot be closed
//     fn close_writer(&mut self) -> Result<(), Box<dyn Error>>;
//
//     fn has_output_stream(&self) -> bool;
//
//     fn encode_header(
//         &mut self,
//         plane_size: PlaneSize,
//         tps: u32,
//         ref_interval: u32,
//         delta_t_max: u32,
//         codec_version: u8,
//         source_camera: Option<SourceCamera>,
//         time_mode: Option<TimeMode>,
//     ) -> Result<(), Box<dyn Error>>;
//     fn get_output_stream_position(&mut self) -> Result<u64, Box<dyn std::error::Error>>;
//
//     fn encode_event(&mut self, event: &Event) -> Result<(), StreamError>;
//     fn encode_events(&mut self, events: &[Event]) -> Result<(), StreamError>;
//     fn encode_events_events(&mut self, events: &[Vec<Event>]) -> Result<(), StreamError>;
//     fn encode_event_v3(&mut self, event: &Event) -> Result<(), StreamError>;
//     fn flush_avu(&mut self) -> Result<(), StreamError>;
// }
//
// pub trait Decoder: Codec {
//     fn decode_header(&mut self) -> Result<usize, Box<dyn Error>>;
//     fn decode_event(&mut self) -> Result<Event, StreamError>;
//
//     fn set_input_stream(&mut self, stream: Option<BufReader<File>>);
//
//     /// Go to this position (as a byte address) in the input stream.
//     /// # Errors
//     /// * If the stream cannot be seeked to the given position
//     /// * If the stream is not seekable
//     /// * If the stream is not open
//     /// * If the given `pos` is not aligned to an [Event]
//     fn set_input_stream_position(&mut self, pos: u64) -> Result<(), StreamError>;
//
//     /// Go to this position (as a byte address) in the input stream, relative to the end
//     /// of the stream
//     /// # Errors
//     /// * If the stream cannot be seeked to the given position
//     /// * If the stream is not seekable
//     /// * If the stream is not open
//     fn set_input_stream_position_from_end(&mut self, pos: i64) -> Result<(), StreamError>;
//
//     /// Get the current position (as a byte address) in the input stream.
//     /// # Errors
//     /// * If the stream is not open
//     fn get_input_stream_position(&mut self) -> Result<u64, Box<dyn Error>>;
//
//     fn get_eof_position(&mut self) -> Result<u64, Box<dyn Error>>;
//
//     /// Close the stream reader safely
//     fn close_reader(&mut self);
// }

use bincode::config::{FixintEncoding, WithOtherEndian, WithOtherIntEncoding};
use bincode::{DefaultOptions, Options};

/// A [`BitWrite`] implementation that discards all data.
pub struct NullWriter;

impl Write for NullWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        Ok(0)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
