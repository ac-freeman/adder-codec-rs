use crate::codec::compressed::fenwick::context_switching::FenwickModel;
use crate::codec::CodecError;
use crate::Event;
use arithmetic_coding::{Decoder, Encoder};
use bitstream_io::{BigEndian, BitReader, BitWriter};
use std::io::Cursor;

pub trait HandleEvent {
    fn ingest_event(&mut self, event: Event);

    fn digest_event(&mut self);
}

trait ComponentCompression {
    fn compress(
        &self,
        encoder: &mut Encoder<FenwickModel, BitWriter<Vec<u8>, BigEndian>>,
        // contexts: &mut Contexts,
        // stream: &mut BitWriter<Vec<u8>, BigEndian>,
        // dtm: DeltaT,
    ) -> Result<(), CodecError>;
    fn decompress(
        decoder: &mut Decoder<FenwickModel, BitReader<Cursor<Vec<u8>>, BigEndian>>,
        // contexts: &mut Contexts,
        // stream: &mut BitReader<Cursor<Vec<u8>>, BigEndian>,
        // dtm: DeltaT,
    ) -> Self;
}
mod event_structure;
