use crate::codec::compressed::fenwick::context_switching::FenwickModel;
use crate::codec::compressed::source_model::cabac_contexts::Contexts;
use crate::codec::compressed::source_model::event_structure::BLOCK_SIZE;
use crate::codec::CodecError;
use crate::{AbsoluteT, DeltaT, Event};
use arithmetic_coding::{Decoder, Encoder};
use bitstream_io::{BigEndian, BitReader, BitWriter};
use std::io::Cursor;

pub trait HandleEvent {
    fn ingest_event(&mut self, event: Event) -> bool;

    fn digest_event(&mut self) -> Result<Event, CodecError>;
    /// Clear out the cube's events and increment the start time by the cube's duration
    fn clear_compression(&mut self);
    fn clear_decompression(&mut self);
}

trait ComponentCompression {
    fn compress(
        &mut self,
        encoder: &mut Encoder<FenwickModel, BitWriter<Vec<u8>, BigEndian>>,
        contexts: &Contexts,
        stream: &mut BitWriter<Vec<u8>, BigEndian>,
        // dtm: DeltaT,
    ) -> Result<(), CodecError>;
    fn decompress(
        decoder: &mut Decoder<FenwickModel, BitReader<Cursor<Vec<u8>>, BigEndian>>,
        contexts: &Contexts,
        stream: &mut BitReader<Cursor<Vec<u8>>, BigEndian>,
        block_idx_y: usize,
        block_idx_x: usize,
        num_channels: usize,
        start_t: AbsoluteT,
        dt_ref: DeltaT,
        num_intervals: usize,
    ) -> Self;
}
pub mod cabac_contexts;
pub mod event_structure;

// fn predict_t_from_d_residual(reference_t: AbsoluteT, d_residual: i16, dt_ref: DeltaT) -> AbsoluteT {
//     reference_t + d_residual as DeltaT * dt_ref
// }
