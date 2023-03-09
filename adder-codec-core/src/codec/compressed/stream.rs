use crate::codec::{CodecError, CodecMetadata, ReadCompression, WriteCompression};
use bitstream_io::{BigEndian, BitRead, BitReader, BitWrite, BitWriter};
use std::io::{Read, Write};

use crate::codec::header::{Magic, MAGIC_COMPRESSED};
use crate::Event;

/// Write compressed ADΔER data to a stream.
pub struct CompressedOutput<W: Write> {
    pub(crate) meta: CodecMetadata,
    pub(crate) stream: Option<BitWriter<W, BigEndian>>,
}

/// Read compressed ADΔER data from a stream.
pub struct CompressedInput<R: Read> {
    pub(crate) meta: CodecMetadata,
    _phantom: std::marker::PhantomData<R>,
}

impl<W: Write> CompressedOutput<W> {
    pub fn new(meta: CodecMetadata, writer: W) -> Self {
        Self {
            meta,
            stream: Some(BitWriter::endian(writer, BigEndian)),
        }
    }
    fn stream(&mut self) -> &mut BitWriter<W, BigEndian> {
        self.stream.as_mut().unwrap()
    }
}

impl<W: Write> WriteCompression<W> for CompressedOutput<W> {
    fn magic(&self) -> Magic {
        MAGIC_COMPRESSED
    }

    fn meta(&self) -> &CodecMetadata {
        &self.meta
    }

    fn meta_mut(&mut self) -> &mut CodecMetadata {
        &mut self.meta
    }

    fn write_bytes(&mut self, bytes: &[u8]) -> Result<(), std::io::Error> {
        self.stream().write_bytes(bytes)
    }

    fn byte_align(&mut self) -> std::io::Result<()> {
        self.stream().byte_align()
    }

    fn into_writer(&mut self) -> Option<W> {
        let tmp = std::mem::replace(&mut self.stream, None);
        match tmp {
            None => None,
            Some(bitwriter) => Some(bitwriter.into_writer()),
        }
    }

    // fn into_writer(self: Self) -> Option<Box<W>> {
    //     Some(Box::new(self.stream.into_writer()))
    // }

    fn flush_writer(&mut self) -> std::io::Result<()> {
        self.stream().flush()
    }

    fn compress(&self, _data: &[u8]) -> Vec<u8> {
        todo!()
    }

    fn ingest_event(&mut self, _event: &Event) -> Result<(), CodecError> {
        todo!()
    }
}

impl<R: Read> CompressedInput<R> {
    pub fn new() -> Self
    where
        Self: Sized,
    {
        Self {
            meta: CodecMetadata {
                codec_version: 0,
                header_size: 0,
                time_mode: Default::default(),
                plane: Default::default(),
                tps: 0,
                ref_interval: 0,
                delta_t_max: 0,
                event_size: 0,
                source_camera: Default::default(),
            },
            // stream: BitReader::endian(reader, BigEndian),
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<R: Read> ReadCompression<R> for CompressedInput<R> {
    fn magic(&self) -> Magic {
        MAGIC_COMPRESSED
    }

    fn meta(&self) -> &CodecMetadata {
        &self.meta
    }

    fn meta_mut(&mut self) -> &mut CodecMetadata {
        &mut self.meta
    }

    fn read_bytes(
        &mut self,
        bytes: &mut [u8],
        reader: &mut BitReader<R, BigEndian>,
    ) -> std::io::Result<()> {
        reader.read_bytes(bytes)
    }

    // fn into_reader(self: Box<Self>, reader: &mut BitReader<R, BigEndian>) -> R {
    //     reader.into_reader()
    // }

    #[allow(unused_variables)]
    fn digest_event(&mut self, reader: &mut BitReader<R, BigEndian>) -> Result<Event, CodecError> {
        todo!()
    }

    #[allow(unused_variables)]
    fn set_input_stream_position(
        &mut self,
        reader: &mut BitReader<R, BigEndian>,
        position: u64,
    ) -> Result<(), CodecError> {
        todo!()
    }
}
