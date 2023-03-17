use crate::codec::{CodecError, CodecMetadata, ReadCompression, WriteCompression};
use arithmetic_coding::{Decoder, Encoder};
use bitstream_io::{BigEndian, BitRead, BitReader, BitWrite, BitWriter};
use std::cmp::min;
use std::io::{Read, Write};

use crate::codec::header::{Magic, MAGIC_COMPRESSED};
use crate::codec_old::compressed::compression::{
    d_residual_default_weights, dt_residual_default_weights, Contexts,
};
use crate::codec_old::compressed::fenwick::context_switching::FenwickModel;
use crate::codec_old::compressed::fenwick::Weights;
use crate::{DeltaT, Event};

/// Write compressed ADΔER data to a stream.
pub struct CompressedOutput<W: Write> {
    pub(crate) meta: CodecMetadata,
    pub(crate) arithmetic_coder:
        Option<arithmetic_coding::Encoder<FenwickModel, BitWriter<W, BigEndian>>>,
    pub(crate) contexts: Option<Contexts>,
    pub(crate) stream: Option<BitWriter<W, BigEndian>>,
}

/// Read compressed ADΔER data from a stream.
pub struct CompressedInput<R: Read> {
    pub(crate) meta: CodecMetadata,
    pub(crate) arithmetic_coder:
        Option<arithmetic_coding::Decoder<FenwickModel, BitReader<R, BigEndian>>>,
    pub(crate) contexts: Option<Contexts>,
    _phantom: std::marker::PhantomData<R>,
}

impl<W: Write> CompressedOutput<W> {
    /// Create a new compressed output stream.
    pub fn new(meta: CodecMetadata, writer: W) -> Self {
        let mut source_model = FenwickModel::with_symbols(
            min(meta.delta_t_max as usize * 2, u16::MAX as usize),
            1 << 30,
        );

        let contexts = Contexts::new(&mut source_model, meta);

        let arithmetic_coder = Encoder::new(source_model);

        Self {
            meta,
            arithmetic_coder: Some(arithmetic_coder),
            contexts: Some(contexts),
            stream: Some(BitWriter::endian(writer, BigEndian)),
        }
    }

    /// Convenience function to get a mutable reference to the underlying stream.
    #[inline(always)]
    pub(crate) fn stream(&mut self) -> &mut BitWriter<W, BigEndian> {
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
        self.arithmetic_coder
            .as_mut()
            .unwrap()
            .model
            .set_context(self.contexts.as_ref().unwrap().eof_context);
        self.arithmetic_coder
            .as_mut()
            .unwrap()
            .encode(None, self.stream.as_mut().unwrap())
            .unwrap();
        // Must flush the encoder to the bitwriter before flushing the bitwriter itself
        self.arithmetic_coder
            .as_mut()
            .unwrap()
            .flush(&mut self.stream.as_mut().unwrap())
            .unwrap();
        self.stream().byte_align().unwrap();
        self.flush_writer().unwrap();
        let tmp = std::mem::replace(&mut self.stream, None);
        tmp.map(|bitwriter| bitwriter.into_writer())
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
    /// Create a new compressed input stream.
    pub fn new(delta_t_max: DeltaT, ref_interval: DeltaT) -> Self
    where
        Self: Sized,
    {
        let mut source_model =
            FenwickModel::with_symbols(min(delta_t_max as usize * 2, u16::MAX as usize), 1 << 30);

        let contexts = Contexts::new(
            &mut source_model,
            CodecMetadata {
                codec_version: 0,
                header_size: 0,
                time_mode: Default::default(),
                plane: Default::default(),
                tps: 0,
                ref_interval,
                delta_t_max,
                event_size: 0,
                source_camera: Default::default(),
            },
        ); // TODO refactor and clean this up

        let arithmetic_coder = Decoder::new(source_model);

        Self {
            meta: CodecMetadata {
                codec_version: 0,
                header_size: 0,
                time_mode: Default::default(),
                plane: Default::default(),
                tps: 0,
                ref_interval,
                delta_t_max,
                event_size: 0,
                source_camera: Default::default(),
            },
            arithmetic_coder: Some(arithmetic_coder),
            contexts: Some(contexts),
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
