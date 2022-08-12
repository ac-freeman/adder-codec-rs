use crate::framer::event_framer::{EventCoordless, SourceType};
use crate::header::EventStreamHeader;
use crate::raw::raw_stream::StreamError;
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::Path;

pub mod framer;
mod header;
pub mod raw;
#[cfg(feature = "transcoder")]
pub mod transcoder; // Have to enable the 'transcoder' feature. Requires OpenCV to be installed.

/// Decimation value; a pixel's sensitivity.
pub type D = u8;

pub const D_SHIFT: [u32; 21] = [
    1, 2, 4, 8, 16, 32, 64, 128, 256, 512, 1024, 2048, 4096, 8192, 16384, 32768, 65536, 131072,
    262144, 524288, 1048576,
];

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub enum SourceCamera {
    #[default]
    FramedU8,
    FramedU16,
    FramedU32,
    FramedU64,
    FramedF32,
    FramedF64,
    Dvs,
    DavisU8,
    Atis,
    Asint,
}

/// The maximum intensity representation for input data. Currently 255 for 8-bit framed input.
pub const MAX_INTENSITY: f32 = 255.0; // TODO: make variable, dependent on input bit depth

/// The default [`D`] value for every pixel at the beginning of transcode
pub const D_START: D = 7;

/// The maximum possible [`D`] value
pub const D_MAX: D = 20;

/// Number of ticks elapsed since a given pixel last fired an [`pixel::Event`]
pub type DeltaT = u32;

/// Large count of ticks (e.g., for tracking the running timestamp of a sequence of events)
pub type BigT = u64;

/// Measure of an amount of light intensity
pub type Intensity = f64;

/// Pixel x- or y- coordinate address in the ADΔER model
pub type PixelAddress = u16;

pub const EOF_PX_ADDRESS: PixelAddress = u16::MAX;

#[derive(Debug, Copy, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Coord {
    pub x: PixelAddress,
    pub y: PixelAddress,
    pub c: Option<u8>,
}

#[derive(Debug, Copy, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct CoordSingle {
    pub x: PixelAddress,
    pub y: PixelAddress,
}

/// An ADΔER event representation
#[derive(Debug, Copy, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Event {
    pub coord: Coord,
    pub d: D,
    pub delta_t: DeltaT,
}

/// An ADΔER event representation
#[derive(Debug, Copy, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct EventSingle {
    pub coord: CoordSingle,
    pub d: D,
    pub delta_t: DeltaT,
}

impl From<&Event> for EventSingle {
    fn from(event: &Event) -> Self {
        EventSingle {
            coord: CoordSingle {
                x: event.coord.x,
                y: event.coord.y,
            },
            d: event.d,
            delta_t: event.delta_t,
        }
    }
}

impl From<EventSingle> for Event {
    fn from(event: EventSingle) -> Self {
        Event {
            coord: Coord {
                x: event.coord.x,
                y: event.coord.y,
                c: None,
            },
            d: event.d,
            delta_t: event.delta_t,
        }
    }
}

pub trait Codec {
    fn new() -> Self;

    fn get_source_type(&self) -> SourceType;

    fn open_writer<P: AsRef<Path>>(&mut self, path: P) -> Result<(), std::io::Error> {
        let file = File::create(&path)?;
        self.set_output_stream(Some(BufWriter::new(file)));
        Ok(())
    }

    fn open_reader<P: AsRef<Path>>(&mut self, path: P) -> Result<(), std::io::Error> {
        let file = File::open(&path)?;
        self.set_input_stream(Some(BufReader::new(file)));
        Ok(())
    }

    fn write_eof(&mut self);
    /// Flush the stream so that program can be exited safely
    fn flush_writer(&mut self);
    fn close_writer(&mut self);

    /// Close the stream so that program can be exited safely
    fn close_reader(&mut self);

    fn set_output_stream(&mut self, stream: Option<BufWriter<File>>);
    fn set_input_stream(&mut self, stream: Option<BufReader<File>>);

    fn encode_header(
        &mut self,
        width: u16,
        height: u16,
        tps: u32,
        ref_interval: u32,
        delta_t_max: u32,
        channels: u8,
        codec_version: u8,
        source_camera: SourceCamera,
    );

    fn decode_header(&mut self) -> Result<(), StreamError>;

    fn encode_event(&mut self, event: &Event);
    fn encode_events(&mut self, events: &[Event]);
    fn encode_events_events(&mut self, events: &[Vec<Event>]);
    fn decode_event(&mut self) -> Result<Event, StreamError>;
}

#[cfg(test)]
mod tests {
    // use crate::EventStreamHeader;
    // use crate::header::MAGIC_RAW;

    #[test]
    fn encode_raw() {}
}
