use crate::codec::compressed::adu::frame::Adu;
use crate::codec::compressed::adu::AduCompression;
use crate::codec::compressed::blocks::prediction::D_RESIDUALS_EMPTY;
use crate::codec::compressed::blocks::{DResidual, BLOCK_SIZE_AREA, D_ENCODE_NO_EVENT};
use crate::codec::compressed::stream::{CompressedInput, CompressedOutput};
use crate::codec::{CodecError, ReadCompression, WriteCompression};
use crate::codec_old::compressed::compression::{
    d_resid_offset, d_resid_offset_inverse, dt_resid_offset, dt_resid_offset_i16,
    dt_resid_offset_i16_inverse, dt_resid_offset_i16_inverse_whole_range,
    dt_resid_offset_i16_whole_range, Contexts, DeltaTResidual, DeltaTResidualSmall,
};
use crate::codec_old::compressed::fenwick::context_switching::FenwickModel;
use crate::{AbsoluteT, DeltaT, D};
use arithmetic_coding::{Decoder, Encoder};
use bitstream_io::{BigEndian, BitReader, BitWrite, BitWriter};
use rand::prelude::StdRng;
use rand::{Rng, SeedableRng};
use std::cmp::min;
use std::io::{Read, Write};
use std::mem;

#[derive(Debug, Clone, PartialEq)]
pub struct AduIntraBlock {
    /// The timestamp of the first event in the ADU.
    pub(crate) head_event_t: AbsoluteT,

    /// The D of the first event in the ADU.
    pub(crate) head_event_d: D,

    /// How many bits the dt_residuals are shifted by.
    pub(crate) shift_loss_param: u8,

    /// Residuals of D between each event and the previous event.
    ///
    /// The first event in the ADU is not included in this array.
    pub(crate) d_residuals: [DResidual; BLOCK_SIZE_AREA],

    /// Residuals of delta_t between each event and the previous event.
    ///
    /// The first event in the ADU is not included in this array.
    pub(crate) dt_residuals: [DeltaTResidualSmall; BLOCK_SIZE_AREA],
}

impl AduCompression for AduIntraBlock {
    fn compress<W: Write>(
        &self,
        encoder: &mut Encoder<FenwickModel, BitWriter<W, BigEndian>>,
        contexts: &mut Contexts,
        stream: &mut BitWriter<W, BigEndian>,
        dtm: DeltaT,
    ) -> Result<(), CodecError> {
        // Get the context references
        let mut d_context = contexts.d_context;
        let mut dt_context = contexts.dt_context;
        let mut dt_context_whole_range = contexts.dt_context_whole_range;
        let mut u8_context = contexts.u8_general_context;

        encoder.model.set_context(u8_context);

        // Write the head event
        for byte in self.head_event_t.to_be_bytes().iter() {
            encoder.encode(Some(&(*byte as usize)), stream).unwrap();
        }

        encoder
            .encode(Some(&(self.head_event_d as usize)), stream)
            .unwrap();

        // Write the shift loss param
        encoder
            .encode(Some(&(self.shift_loss_param as usize)), stream)
            .unwrap();

        // Write the d_residuals
        compress_d_residuals(&self.d_residuals, encoder, d_context, stream);

        // Write the dt_residuals
        compress_dt_residuals(
            &self.dt_residuals,
            encoder,
            dt_context_whole_range,
            stream,
            dtm,
        );

        Ok(())
    }

    fn decompress<R: Read>(
        stream: &mut BitReader<R, BigEndian>,
        input: &mut CompressedInput<R>,
    ) -> Self {
        // Initialize empty intra block
        let mut intra_block = Self {
            head_event_t: 0,
            head_event_d: 0,
            shift_loss_param: 0,
            d_residuals: D_RESIDUALS_EMPTY,
            dt_residuals: [0; BLOCK_SIZE_AREA],
        };

        // Get the context references
        let mut decoder = input.arithmetic_coder.as_mut().unwrap();
        let mut d_context = input.contexts.as_mut().unwrap().d_context;
        let mut dt_context_whole_range = input.contexts.as_mut().unwrap().dt_context_whole_range;
        let mut dt_context = input.contexts.as_mut().unwrap().dt_context;
        let mut u8_context = input.contexts.as_mut().unwrap().u8_general_context;

        decoder.model.set_context(u8_context);

        // Read the head event
        let mut bytes = [0; mem::size_of::<AbsoluteT>()];
        for byte in bytes.iter_mut() {
            *byte = decoder.decode(stream).unwrap().unwrap() as u8;
        }
        intra_block.head_event_t = AbsoluteT::from_be_bytes(bytes);

        intra_block.head_event_d = decoder.decode(stream).unwrap().unwrap() as D;

        // Read the shift loss param
        intra_block.shift_loss_param = decoder.decode(stream).unwrap().unwrap() as u8;

        // Read the d_residuals
        decompress_d_residuals(&mut intra_block.d_residuals, decoder, d_context, stream);

        // Read the dt_residuals
        decompress_dt_residuals(
            &mut intra_block.dt_residuals,
            decoder,
            dt_context_whole_range,
            stream,
            input.meta.delta_t_max,
        );

        intra_block
    }

    fn decompress_debug<R: Read>(
        stream: &mut BitReader<R, BigEndian>,
        input: &mut CompressedInput<R>,
        reference_adu: &Adu,
    ) -> Self {
        todo!()
    }
}

pub fn compress_d_residuals<W: Write>(
    d_residuals: &[DResidual; BLOCK_SIZE_AREA],
    encoder: &mut Encoder<FenwickModel, BitWriter<W, BigEndian>>,
    d_context: usize,
    stream: &mut BitWriter<W, BigEndian>,
) -> Result<(), CodecError> {
    encoder.model.set_context(d_context);
    for d_residual in d_residuals.iter() {
        encoder.encode(Some(&d_resid_offset(*d_residual)), stream)?;
    }
    Ok(())
}

pub fn decompress_d_residuals<R: Read>(
    d_residuals: &mut [DResidual; BLOCK_SIZE_AREA],
    decoder: &mut Decoder<FenwickModel, BitReader<R, BigEndian>>,
    d_context: usize,
    stream: &mut BitReader<R, BigEndian>,
) {
    decoder.model.set_context(d_context);
    for d_residual in d_residuals.iter_mut() {
        let symbol = decoder.decode(stream).unwrap();
        *d_residual = d_resid_offset_inverse(symbol.unwrap());
    }
}

fn compress_dt_residuals<W: Write>(
    dt_residuals: &[DeltaTResidualSmall; BLOCK_SIZE_AREA],
    encoder: &mut Encoder<FenwickModel, BitWriter<W, BigEndian>>,
    dt_context_whole_range: usize,
    stream: &mut BitWriter<W, BigEndian>,
    delta_t_max: DeltaT,
) -> Result<(), CodecError> {
    encoder.model.set_context(dt_context_whole_range);
    for dt_residual in dt_residuals.iter() {
        encoder.encode(
            Some(&dt_resid_offset_i16_whole_range(*dt_residual, delta_t_max)),
            stream,
        )?;
    }
    Ok(())
}

fn decompress_dt_residuals<R: Read>(
    dt_residuals: &mut [DeltaTResidualSmall; BLOCK_SIZE_AREA],
    decoder: &mut Decoder<FenwickModel, BitReader<R, BigEndian>>,
    dt_context_whole_range: usize,
    stream: &mut BitReader<R, BigEndian>,
    delta_t_max: DeltaT,
) {
    decoder.model.set_context(dt_context_whole_range);
    for dt_residual in dt_residuals.iter_mut() {
        let symbol = decoder.decode(stream).unwrap();
        *dt_residual = dt_resid_offset_i16_inverse_whole_range(symbol.unwrap(), delta_t_max);
    }
}

/// Generate an intra block with random event data
pub fn gen_random_intra_block(min_t: AbsoluteT, dtm: DeltaT, seed: Option<u64>) -> AduIntraBlock {
    let mut rng = match seed {
        None => StdRng::from_rng(rand::thread_rng()).unwrap(),
        Some(num) => StdRng::seed_from_u64(num),
    };

    let mut d_residuals = D_RESIDUALS_EMPTY;
    let mut dt_residuals: [DeltaTResidualSmall; BLOCK_SIZE_AREA] = [0; BLOCK_SIZE_AREA];

    let mut block = AduIntraBlock {
        head_event_t: rng.gen_range(min_t..=min_t + dtm),
        head_event_d: rng.gen_range(0..=127),
        shift_loss_param: rng.gen_range(0..=3),
        d_residuals,
        dt_residuals,
    };

    let end = min(dtm, i16::MAX as DeltaT);
    // skip the first event index
    for i in 1..BLOCK_SIZE_AREA {
        block.d_residuals[i] = rng.gen_range(-255..=255);

        block.dt_residuals[i] =
            rng.gen_range(-(end as DeltaTResidualSmall)..=(end as DeltaTResidualSmall));
    }

    block
}

#[cfg(test)]
mod tests {
    use crate::codec::compressed::adu::intrablock::{gen_random_intra_block, AduIntraBlock};
    use crate::codec::compressed::adu::AduCompression;
    use crate::codec::compressed::blocks::BLOCK_SIZE_AREA;
    use crate::codec::compressed::stream::CompressedInput;
    use crate::codec::{CodecMetadata, WriteCompression};
    use itertools::izip;
    use std::error::Error;
    use std::io::BufReader;

    fn setup_encoder() -> crate::codec::compressed::stream::CompressedOutput<Vec<u8>> {
        let meta = CodecMetadata {
            delta_t_max: 100,
            ref_interval: 100,
            ..Default::default()
        };
        // By building the CompressedOutput directly (rather than calling Encoder::new_compressed),
        // we can avoid writing the header and stuff for testing purposes.
        crate::codec::compressed::stream::CompressedOutput::new(meta, Vec::new())
    }

    fn setup_encoder_bigdtm() -> crate::codec::compressed::stream::CompressedOutput<Vec<u8>> {
        let meta = CodecMetadata {
            delta_t_max: 10000000,
            ref_interval: 100,
            ..Default::default()
        };
        // By building the CompressedOutput directly (rather than calling Encoder::new_compressed),
        // we can avoid writing the header and stuff for testing purposes.
        crate::codec::compressed::stream::CompressedOutput::new(meta, Vec::new())
    }

    fn compress_intra_block() -> Result<(AduIntraBlock, Vec<u8>), Box<dyn Error>> {
        let mut encoder = setup_encoder();
        let intra_block = gen_random_intra_block(1234, encoder.meta.delta_t_max, Some(7));

        assert!(intra_block
            .compress(
                encoder.arithmetic_coder.as_mut().unwrap(),
                encoder.contexts.as_mut().unwrap(),
                encoder.stream.as_mut().unwrap(),
                encoder.meta.delta_t_max
            )
            .is_ok());

        let written_data = encoder.into_writer().unwrap();

        Ok((intra_block, written_data))
    }

    fn compress_intra_block_bigdtm() -> Result<(AduIntraBlock, Vec<u8>), Box<dyn Error>> {
        let mut encoder = setup_encoder_bigdtm();
        let intra_block = gen_random_intra_block(1234, encoder.meta.delta_t_max, Some(7));

        assert!(intra_block
            .compress(
                encoder.arithmetic_coder.as_mut().unwrap(),
                encoder.contexts.as_mut().unwrap(),
                encoder.stream.as_mut().unwrap(),
                encoder.meta.delta_t_max
            )
            .is_ok());

        let written_data = encoder.into_writer().unwrap();

        Ok((intra_block, written_data))
    }

    #[test]
    fn test_compress_intra_block() {
        let (_, written_data) = compress_intra_block().unwrap();
        let output_len = written_data.len();
        let input_len = 1028; // Rough approximation
        assert!(output_len < input_len);
        eprintln!("Written data: {:?}", written_data);
    }

    #[test]
    fn test_decompress_intra_block() {
        let (intra_block, written_data) = compress_intra_block().unwrap();
        let tmp_len = written_data.len();

        let mut bufreader = BufReader::new(written_data.as_slice());
        let mut bitreader =
            bitstream_io::BitReader::endian(&mut bufreader, bitstream_io::BigEndian);

        let mut decoder = CompressedInput::new(100, 100);

        let decoded_intra_block = AduIntraBlock::decompress(&mut bitreader, &mut decoder);

        decoder
            .arithmetic_coder
            .as_mut()
            .unwrap()
            .model
            .set_context(decoder.contexts.as_mut().unwrap().eof_context);
        let eof = decoder
            .arithmetic_coder
            .as_mut()
            .unwrap()
            .decode(&mut bitreader)
            .unwrap();
        assert!(eof.is_none());
        assert_eq!(intra_block.head_event_t, decoded_intra_block.head_event_t);
        assert_eq!(intra_block.head_event_d, decoded_intra_block.head_event_d);
        assert_eq!(
            intra_block.shift_loss_param,
            decoded_intra_block.shift_loss_param
        );
        assert_eq!(intra_block.d_residuals, decoded_intra_block.d_residuals);
        assert_eq!(intra_block.dt_residuals, decoded_intra_block.dt_residuals);
    }

    #[test]
    fn test_decompress_intra_block_bigdtm() {
        let (intra_block, written_data) = compress_intra_block_bigdtm().unwrap();
        let tmp_len = written_data.len();

        let mut bufreader = BufReader::new(written_data.as_slice());
        let mut bitreader =
            bitstream_io::BitReader::endian(&mut bufreader, bitstream_io::BigEndian);

        let mut decoder = CompressedInput::new(10000000, 100);

        let decoded_intra_block = AduIntraBlock::decompress(&mut bitreader, &mut decoder);

        decoder
            .arithmetic_coder
            .as_mut()
            .unwrap()
            .model
            .set_context(decoder.contexts.as_mut().unwrap().eof_context);
        let eof = decoder
            .arithmetic_coder
            .as_mut()
            .unwrap()
            .decode(&mut bitreader)
            .unwrap();
        assert!(eof.is_none());
        assert_eq!(intra_block.head_event_t, decoded_intra_block.head_event_t);
        assert_eq!(intra_block.head_event_d, decoded_intra_block.head_event_d);
        assert_eq!(
            intra_block.shift_loss_param,
            decoded_intra_block.shift_loss_param
        );
        assert_eq!(intra_block.d_residuals, decoded_intra_block.d_residuals);
        assert_eq!(intra_block.dt_residuals, decoded_intra_block.dt_residuals);
    }
}