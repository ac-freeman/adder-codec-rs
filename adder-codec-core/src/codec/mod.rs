use crate::codec::header::Magic;
use crate::{DeltaT, Event, PlaneSize, SourceCamera, TimeMode};
use bitstream_io::{BigEndian, BitReader};
use std::io;
use std::io::{Read, Write};

pub mod compressed;
pub mod decoder;
pub mod empty;
pub mod encoder;
mod header;
pub mod raw;

pub const LATEST_CODEC_VERSION: u8 = 2;

#[derive(Copy, Clone)]
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

    fn meta(&self) -> &CodecMetadata;

    fn meta_mut(&mut self) -> &mut CodecMetadata;

    fn write_bytes(&mut self, bytes: &[u8]) -> io::Result<()>;

    fn byte_align(&mut self) -> io::Result<()>;

    /// Consumes the compression stream and returns the underlying writer.
    fn into_writer(self: Box<Self>) -> Option<W>;

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

// unsafe impl<R: Read> Send for ReadCompression {}

use thiserror::Error;

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

    #[error("Bincode error")]
    BincodeError(#[from] bincode::Error),

    #[error("IO error")]
    IoError(#[from] io::Error),

    #[error("Plane error")]
    PlaneError(#[from] crate::PlaneError),
}
