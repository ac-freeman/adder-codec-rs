use crate::codec::compressed::adu::frame::Adu;
use crate::codec::compressed::stream::{CompressedInput, CompressedOutput};
use crate::codec::CodecError;
use crate::codec_old::compressed::compression::Contexts;
use crate::codec_old::compressed::fenwick::context_switching::FenwickModel;
use crate::DeltaT;
use arithmetic_coding::{Decoder, Encoder};
use bitstream_io::{BigEndian, BitReader, BitWrite, BitWriter};
use std::io::{Cursor, Read, Write};

pub mod cube;
pub mod frame;
pub mod interblock;
pub mod intrablock;

pub trait AduCompression {
    fn compress<W: Write>(
        &self,
        encoder: &mut Encoder<FenwickModel, BitWriter<Vec<u8>, BigEndian>>,
        contexts: &mut Contexts,
        stream: &mut BitWriter<W, BigEndian>,
        dtm: DeltaT,
        ref_interval: DeltaT,
    ) -> Result<(), CodecError>;
    fn decompress<R: Read>(
        decoder: &mut Decoder<FenwickModel, BitReader<Cursor<Vec<u8>>, BigEndian>>,
        contexts: &mut Contexts,
        stream: &mut BitReader<R, BigEndian>,
        dtm: DeltaT,
        ref_interval: DeltaT,
    ) -> Self;
}

pub trait AduComponentCompression {
    fn compress(
        &self,
        encoder: &mut Encoder<FenwickModel, BitWriter<Vec<u8>, BigEndian>>,
        contexts: &mut Contexts,
        stream: &mut BitWriter<Vec<u8>, BigEndian>,
        dtm: DeltaT,
    ) -> Result<(), CodecError>;
    fn decompress(
        decoder: &mut Decoder<FenwickModel, BitReader<Cursor<Vec<u8>>, BigEndian>>,
        contexts: &mut Contexts,
        stream: &mut BitReader<Cursor<Vec<u8>>, BigEndian>,
        dtm: DeltaT,
    ) -> Self;
}

/// Only use for running tests, where the output.stream is what we're writing our arithmetic codes
/// to (directly).
fn add_eof(output: &mut CompressedOutput<Vec<u8>>) {
    let mut encoder = output.arithmetic_coder.as_mut().unwrap();
    let mut stream = output.stream.as_mut().unwrap();
    let eof_context = output.contexts.as_mut().unwrap().eof_context;
    encoder.model.set_context(eof_context);
    encoder.encode(None, stream).unwrap();
    encoder.flush(stream).unwrap();
    stream.byte_align().unwrap();
    stream.flush().unwrap();
}
