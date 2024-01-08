#![warn(missing_docs)]

use crate::codec::header::Magic;
use crate::{DeltaT, Event, PlaneSize, SourceCamera, TimeMode};
use bitstream_io::{BigEndian, BitReader};
use enum_dispatch::enum_dispatch;
use std::io;
use std::io::{Read, Seek, Sink, Write};

/// Different options for what to with the events we're given
#[enum_dispatch(WriteCompression<W>)]
pub enum WriteCompressionEnum<W: Write> {
    /// Perform (possibly lossy) compression on the ADΔER stream, and arithmetic coding
    #[cfg(feature = "compression")]
    CompressedOutput(CompressedOutput<W>),

    /// Write the ADΔER stream as raw events
    RawOutput(RawOutput<W>),

    /// An empty output stream. Send all the data into the void.
    EmptyOutput(EmptyOutput<Sink>),
}

/// The encoder type, along with any associated options
#[derive(Default, Clone, Copy, PartialEq, Debug)]
pub enum EncoderType {
    /// Perform (possibly lossy) compression on the ADΔER stream, and arithmetic coding
    Compressed,

    /// Write the ADΔER stream as raw events
    Raw,

    // /// Write the ADΔER stream as raw events, but make sure that they are ordered perfectly according
    // /// to their firing times
    // RawInterleaved,
    // RawBandwidthLimited {
    //     target_event_rate: f64,
    //     alpha: f64,
    // },
    /// Do not write any data to the output stream
    #[default]
    Empty,
}

#[enum_dispatch(ReadCompression<R>)]
enum ReadCompressionEnum<R: Read + Seek> {
    #[cfg(feature = "compression")]
    CompressedInput(CompressedInput<R>),
    RawInput(RawInput<R>),
}

/// Compressed codec utilities
#[cfg(feature = "compression")]
pub mod compressed;

/// ADΔER stream decoder
pub mod decoder;

/// Filler for when generated ADΔER events need not be captured
pub mod empty;

/// ADΔER stream encoder
pub mod encoder;
mod header;

pub mod rate_controller;
/// Raw codec utilities
pub mod raw;

/// Current latest version of the codec.
///
/// This is the version which will be written to the header.
pub const LATEST_CODEC_VERSION: u8 = 3;

/// The metadata which stays the same over the course of an ADΔER stream
#[allow(missing_docs)]
#[derive(Copy, Clone, Debug)]
pub struct CodecMetadata {
    pub codec_version: u8,
    pub header_size: usize,
    pub time_mode: TimeMode,
    pub plane: PlaneSize,
    pub tps: DeltaT,
    pub ref_interval: DeltaT,
    pub delta_t_max: DeltaT,
    pub event_size: u8,
    pub source_camera: SourceCamera,
    pub adu_interval: usize, // TODO: Allow the adu_interval to be non-constant. Each ADU will encode its own size at its beginning
}

impl Default for CodecMetadata {
    fn default() -> Self {
        CodecMetadata {
            codec_version: LATEST_CODEC_VERSION,
            header_size: 24,
            time_mode: Default::default(),
            plane: Default::default(),
            tps: 2550,
            ref_interval: 255,
            delta_t_max: 255,
            event_size: 9,
            source_camera: Default::default(),
            adu_interval: 1,
        }
    }
}

/// A trait for writing ADΔER data to a stream.
#[enum_dispatch]
pub trait WriteCompression<W: Write> {
    // /// A struct implementing `WriteCompression` should take ownership of the `writer`.
    // fn new(meta: CodecMetadata, writer: W) -> Self
    // where
    //     Self: Sized;

    /// The magic number for this compression format.
    fn magic(&self) -> Magic;

    /// Returns a reference to the metadata
    fn meta(&self) -> &CodecMetadata;

    /// Returns a mutable reference to the metadata
    fn meta_mut(&mut self) -> &mut CodecMetadata;

    // fn stream(&mut self) -> &mut W;

    /// Write the given bytes to the stream
    fn write_bytes(&mut self, bytes: &[u8]) -> Result<(), std::io::Error>;

    /// Align the bitstream to the next byte boundary
    fn byte_align(&mut self) -> io::Result<()>;

    /// Consumes the compression stream and returns the underlying writer.
    fn into_writer(&mut self) -> Option<W>;

    /// Flush the `BitWriter`. Does not flush the internal `BufWriter`.
    fn flush_writer(&mut self) -> io::Result<()>;

    /// Take in an event and process it. May or may not write to the output, depending on the state
    /// of the stream (Is it ready to write events? Is it accumulating/reorganizing events? etc.)
    fn ingest_event(&mut self, event: Event) -> Result<(), CodecError>;

    // #[cfg(feature = "compression")]
    // fn ingest_event_debug(&mut self, event: Event) -> Result<Option<Adu>, CodecError>;
}

/// A trait for reading ADΔER data from a stream.
///
/// A struct implementing `ReadCompression` does not take ownership of the read handle.
/// Subsequent calls to the compressor will pass the read handle each time. The caller is
/// responsible for maintaining the reader.
#[enum_dispatch]
pub trait ReadCompression<R: Read> {
    // fn new() -> Self
    // where
    //     Self: Sized;

    /// Returns the magic number for the codec
    fn magic(&self) -> Magic;

    /// Returns a reference to the metadata
    fn meta(&self) -> &CodecMetadata;

    /// Returns a mutable reference to the metadata
    fn meta_mut(&mut self) -> &mut CodecMetadata;

    /// Read a certain number of bytes from the stream, indicated by the size of the buffer passed.
    fn read_bytes(
        &mut self,
        bytes: &mut [u8],
        reader: &mut BitReader<R, BigEndian>,
    ) -> io::Result<()>;
    // fn into_reader(self: Box<Self>, reader: &mut BitReader<R, BigEndian>) -> R;

    /// Read the next event from the stream. Returns `None` if the stream is exhausted.
    fn digest_event(&mut self, reader: &mut BitReader<R, BigEndian>) -> Result<Event, CodecError>;

    // #[cfg(feature = "compression")]
    // fn digest_event_debug(
    //     &mut self,
    //     reader: &mut BitReader<R, BigEndian>,
    // ) -> Result<(Option<Adu>, Event), CodecError>;

    /// Set the input stream position to the given byte offset.
    fn set_input_stream_position(
        &mut self,
        reader: &mut BitReader<R, BigEndian>,
        position: u64,
    ) -> Result<(), CodecError>;

    // fn byte_align(&mut self) -> io::Result<()>;

    // fn decompress(&self, data: &[u8]) -> Vec<u8>;
}

// unsafe impl<R: Read> Send for ReadCompression {}
// #[cfg(feature = "compression")]
// use crate::codec::compressed::adu::frame::Adu;
#[cfg(feature = "compression")]
use crate::codec::compressed::stream::{CompressedInput, CompressedOutput};
use crate::codec::empty::stream::EmptyOutput;
use crate::codec::rate_controller::Crf;
use crate::codec::raw::stream::{RawInput, RawOutput};
use thiserror::Error;

#[allow(missing_docs)]
#[derive(Error, Debug)]
pub enum CodecError {
    #[error("stream has not been initialized")]
    UnitializedStream,

    #[error("Reached end of file when expected")]
    Eof,

    #[error("Could not deserialize data. EOF reached at unexpected time.")]
    Deserialize,

    #[error("File formatted incorrectly")]
    BadFile,

    #[error("File is of unexpected type (compressed or raw)")]
    WrongMagic,

    #[error("Attempted to seek to a bad position in the stream")]
    Seek,

    #[error("Unsupported codec version (expected {LATEST_CODEC_VERSION} or lower, found {0})")]
    UnsupportedVersion(u8),

    #[error("Malformed encoder")]
    MalformedEncoder,

    #[error("Bincode error")]
    BincodeError(#[from] bincode::Error),

    #[error("IO error")]
    IoError(#[from] io::Error),

    #[error("Plane error")]
    PlaneError(#[from] crate::PlaneError),

    // #[cfg(feature = "compression")]
    // #[error("Blocking error")]
    // BlockError(#[from] crate::codec::compressed::blocks::block::BlockError),
    #[cfg(feature = "compression")]
    #[error("Arithmetic coding error")]
    ArithmeticCodingError(#[from] arithmetic_coding_adder_dep::Error),

    /// Vision application error
    #[error("Vision application error")]
    VisionError(String),

    #[error("No more events to read")]
    NoMoreEvents,
}

/*
Encoder options below
 */

/// Options related to encoder controls
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct EncoderOptions {
    /// Allow the encoder to randomly drop events before compressing, if the event rate is too high
    pub event_drop: EventDrop,

    /// Reorder the events according to their firing times
    pub event_order: EventOrder,

    pub crf: Crf,
}

impl EncoderOptions {
    pub fn default(plane: PlaneSize) -> Self {
        Self {
            event_drop: Default::default(),
            event_order: Default::default(),
            crf: Crf::new(None, plane),
        }
    }
}

/// Allow the encoder to randomly drop events before compressing, if the event rate is too high
#[derive(Default, Copy, Clone, PartialEq, Debug)]
pub enum EventDrop {
    /// Don't drop any events
    #[default]
    None,

    /// Randomly drop events according to this user-provided event rate
    Manual {
        target_event_rate: f64,

        /// The decay rate in [0., 1.]
        alpha: f64,
    },

    /// TODO: Implement this. Query the actual network bandwidth accoring to some stream handle
    /// and drop events accordingly.
    Auto,
}

/// Reorder the events according to their firing times
#[derive(Default, Copy, Clone, PartialEq, Debug)]
pub enum EventOrder {
    /// Pass on the events in the order they're received in
    #[default]
    Unchanged,

    /// Reorder the events according to their firing times
    Interleaved,
}
