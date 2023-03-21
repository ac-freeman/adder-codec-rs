use crate::codec::compressed::adu::frame::Adu;
use crate::codec::compressed::stream::{CompressedInput, CompressedOutput};
use crate::codec::CodecError;
use crate::codec_old::compressed::compression::Contexts;
use crate::codec_old::compressed::fenwick::context_switching::FenwickModel;
use crate::DeltaT;
use arithmetic_coding::Encoder;
use bitstream_io::{BigEndian, BitReader, BitWriter};
use std::io::{Read, Write};

pub mod cube;
pub mod frame;
pub mod interblock;
pub mod intrablock;

pub trait AduCompression {
    fn compress<W: Write>(
        &self,
        encoder: &mut Encoder<FenwickModel, BitWriter<W, BigEndian>>,
        contexts: &mut Contexts,
        stream: &mut BitWriter<W, BigEndian>,
        dtm: DeltaT,
    ) -> Result<(), CodecError>;
    fn decompress<R: Read>(
        stream: &mut BitReader<R, BigEndian>,
        input: &mut CompressedInput<R>,
    ) -> Self;

    fn decompress_debug<R: Read>(
        stream: &mut BitReader<R, BigEndian>,
        input: &mut CompressedInput<R>,
        reference_adu: &Adu,
    ) -> Self;
}
