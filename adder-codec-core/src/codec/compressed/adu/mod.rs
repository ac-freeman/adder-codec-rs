use crate::codec::compressed::adu::frame::Adu;
use crate::codec::compressed::stream::{CompressedInput, CompressedOutput};
use crate::codec::CodecError;
use crate::codec_old::compressed::compression::Contexts;
use crate::codec_old::compressed::fenwick::context_switching::FenwickModel;
use crate::DeltaT;
use arithmetic_coding::{Decoder, Encoder};
use bitstream_io::{BigEndian, BitReader, BitWriter};
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
    ) -> Result<(), CodecError>;
    fn decompress<R: Read>(
        decoder: &mut Decoder<FenwickModel, BitReader<Cursor<Vec<u8>>, BigEndian>>,
        contexts: &mut Contexts,
        stream: &mut BitReader<R, BigEndian>,
        dtm: DeltaT,
    ) -> Self;

    fn decompress_debug<R: Read>(
        stream: &mut BitReader<R, BigEndian>,
        input: &mut CompressedInput<R>,
        reference_adu: &Adu,
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

    fn decompress_debug<R: Read>(
        stream: &mut BitReader<R, BigEndian>,
        input: &mut CompressedInput<R>,
        reference_adu: &Adu,
    ) -> Self;
}
