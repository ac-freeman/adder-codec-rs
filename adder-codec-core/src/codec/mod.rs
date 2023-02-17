use crate::codec::header::Magic;
use crate::{DeltaT, Event, PlaneSize, SourceCamera, TimeMode};
use bitstream_io::{BigEndian, BitReader};
use std::io::{Read, Write};
use std::{fmt, io};

pub mod compressed;
pub mod decoder;
pub mod empty;
pub mod encoder;
mod header;
pub mod raw;

pub const LATEST_CODEC_VERSION: u8 = 2;

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
        }
    }
}

pub trait WriteCompression<W: Write> {
    /// A struct implementing `WriteCompression` should take ownership of the `writer`.
    fn new(meta: CodecMetadata, writer: W) -> Self
    where
        Self: Sized;

    fn magic(&self) -> Magic;

    #[inline]
    fn meta(&self) -> &CodecMetadata;

    fn meta_mut(&mut self) -> &mut CodecMetadata;

    fn write_bytes(&mut self, bytes: &[u8]) -> io::Result<()>;

    fn byte_align(&mut self) -> io::Result<()>;

    /// Consumes the compression stream and returns the underlying writer.
    fn into_writer(self: Box<Self>) -> W;

    /// Flush the `BitWriter`. Does not flush the internal `BufWriter`.
    fn flush_writer(&mut self) -> io::Result<()>;

    fn compress(&self, data: &[u8]) -> Vec<u8>;

    /// Take in an event and process it. May or may not write to the output, depending on the state
    /// of the stream (Is it ready to write events? Is it accumulating/reorganizing events? etc.)
    fn ingest_event(&mut self, event: &Event) -> Result<(), CodecError>;
}

pub trait ReadCompression<R: Read> {
    /// A struct implementing `ReadCompression` does not take ownership of the read handle.
    /// Subsequent calls to the compressor will pass the read handle each time. The caller is
    /// responsible for maintaining the reader.
    fn new() -> Self
    where
        Self: Sized;

    fn magic(&self) -> Magic;

    #[inline]
    fn meta(&self) -> &CodecMetadata;

    fn meta_mut(&mut self) -> &mut CodecMetadata;

    fn read_bytes(
        &mut self,
        bytes: &mut [u8],
        reader: &mut BitReader<R, BigEndian>,
    ) -> io::Result<()>;
    // fn into_reader(self: Box<Self>, reader: &mut BitReader<R, BigEndian>) -> R;

    fn digest_event(&mut self, reader: &mut BitReader<R, BigEndian>) -> Result<Event, CodecError>;

    /// Set the input stream position to the given byte offset.
    fn set_input_stream_position(
        &mut self,
        reader: &mut BitReader<R, BigEndian>,
        position: u64,
    ) -> Result<(), CodecError>;

    // fn byte_align(&mut self) -> io::Result<()>;

    // fn decompress(&self, data: &[u8]) -> Vec<u8>;
}

#[derive(Debug)]
pub enum CodecError {
    /// Stream has not been initialized
    UnitializedStream,

    /// Reached end of file when expected
    Eof,

    /// Could not deserialize data. EOF reached at unexpected time.
    Deserialize,

    /// File formatted incorrectly
    BadFile,

    /// Attempted to seek to a bad position in the stream
    Seek,

    /// Unsupported codec version
    UnsupportedVersion(u8),

    /// Bincode error
    BincodeError(bincode::Error),
}

impl fmt::Display for CodecError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        // write!(f, "Stream error")
        write!(f, "{self:?}")
    }
}

impl From<CodecError> for Box<dyn std::error::Error> {
    fn from(value: CodecError) -> Self {
        value.to_string().into()
    }
}

impl From<Box<bincode::ErrorKind>> for CodecError {
    fn from(value: Box<bincode::ErrorKind>) -> Self {
        CodecError::BincodeError(value)
    }
}
