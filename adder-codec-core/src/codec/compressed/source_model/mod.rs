use crate::codec::compressed::fenwick::context_switching::FenwickModel;
use crate::codec::compressed::source_model::cabac_contexts::Contexts;
use crate::codec::CodecError;
use crate::{AbsoluteT, Event, EventCoordless};
use arithmetic_coding_adder_dep::{Decoder, Encoder};
use bitstream_io::{BigEndian, BitReader, BitWrite, BitWriter};
use std::io::Cursor;

pub trait HandleEvent {
    fn ingest_event(&mut self, event: Event) -> bool;

    fn digest_event(&mut self) -> Result<Event, CodecError>;
    /// Clear out the cube's events and increment the start time by the cube's duration
    fn clear_compression(&mut self);
    fn clear_decompression(&mut self);
}

trait ComponentCompression {
    fn compress_intra(
        &mut self,
        encoder: &mut Encoder<FenwickModel, BitWriter<Vec<u8>, BigEndian>>,
        contexts: &Contexts,
        stream: &mut BitWriter<Vec<u8>, BigEndian>,
        init_event: &mut Option<EventCoordless>,
        threshold_option: Option<u8>,
    ) -> Result<(), CodecError>;
    fn decompress_intra(
        &mut self,
        decoder: &mut Decoder<FenwickModel, BitReader<Cursor<Vec<u8>>, BigEndian>>,
        contexts: &Contexts,
        stream: &mut BitReader<Cursor<Vec<u8>>, BigEndian>,
        start_t: AbsoluteT,
        init_event: &mut Option<EventCoordless>,
    );
    fn decompress_inter(
        &mut self,
        decoder: &mut Decoder<FenwickModel, BitReader<Cursor<Vec<u8>>, BigEndian>>,
        contexts: &Contexts,
        stream: &mut BitReader<Cursor<Vec<u8>>, BigEndian>,
    );
    fn compress_inter(
        &mut self,
        encoder: &mut Encoder<FenwickModel, BitWriter<Vec<u8>, BigEndian>>,
        contexts: &Contexts,
        stream: &mut BitWriter<Vec<u8>, BigEndian>,
        c_thresh_max: Option<u8>,
    ) -> Result<(), CodecError>;
}
pub mod cabac_contexts;
pub mod event_structure;

// fn predict_t_from_d_residual(reference_t: AbsoluteT, d_residual: i16, dt_ref: DeltaT) -> AbsoluteT {
//     reference_t + d_residual as DeltaT * dt_ref
// }
