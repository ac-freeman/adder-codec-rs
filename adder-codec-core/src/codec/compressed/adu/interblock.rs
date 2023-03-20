use crate::codec::compressed::adu::intrablock::{
    compress_d_residuals, compress_dt_residuals, decompress_d_residuals, decompress_dt_residuals,
};
use crate::codec::compressed::adu::AduCompression;
use crate::codec::compressed::blocks::prediction::D_RESIDUALS_EMPTY;
use crate::codec::compressed::blocks::{DResidual, BLOCK_SIZE_AREA};
use crate::codec::compressed::stream::{CompressedInput, CompressedOutput};
use crate::codec::{CodecError, ReadCompression, WriteCompression};
use crate::codec_old::compressed::compression::Contexts;
use crate::codec_old::compressed::fenwick::context_switching::FenwickModel;
use crate::DeltaT;
use arithmetic_coding::Encoder;
use bitstream_io::{BigEndian, BitRead, BitReader, BitWrite, BitWriter};
use std::io::{Error, Read, Write};

pub struct AduInterBlock {
    /// How many bits the dt_residuals are shifted by.
    pub(crate) shift_loss_param: u8,

    /// Prediction residuals of D between each event and the event in the previous block.
    pub(crate) d_residuals: [DResidual; BLOCK_SIZE_AREA],

    /// Prediction residuals of delta_t between each event and the event in the previous block.
    pub(crate) t_residuals: [i16; BLOCK_SIZE_AREA],
}

impl AduCompression for AduInterBlock {
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
        let mut u8_context = contexts.u8_general_context;

        encoder.model.set_context(u8_context);

        // Write the shift loss parameter.
        encoder.encode(Some(&(self.shift_loss_param as usize)), stream)?;

        // Write the d_residuals
        compress_d_residuals(&self.d_residuals, encoder, d_context, stream);

        // Write the dt_residuals
        compress_dt_residuals(&self.t_residuals, encoder, dt_context, stream, dtm);

        Ok(())
    }

    fn decompress<R: Read>(
        stream: &mut BitReader<R, BigEndian>,
        input: &mut CompressedInput<R>,
    ) -> Self {
        // Initialize empty inter block
        let mut inter_block = Self {
            shift_loss_param: 0,
            d_residuals: D_RESIDUALS_EMPTY,
            t_residuals: [0; BLOCK_SIZE_AREA],
        };

        // Get the context references
        let mut decoder = input.arithmetic_coder.as_mut().unwrap();
        let mut d_context = input.contexts.as_mut().unwrap().d_context;
        let mut dt_context = input.contexts.as_mut().unwrap().dt_context;
        let mut u8_context = input.contexts.as_mut().unwrap().u8_general_context;

        decoder.model.set_context(u8_context);

        // Read the shift loss parameter.
        inter_block.shift_loss_param = decoder.decode(stream).unwrap().unwrap() as u8;

        // Read the d_residuals
        decompress_d_residuals(&mut inter_block.d_residuals, decoder, d_context, stream);

        // Read the dt_residuals
        decompress_dt_residuals(
            &mut inter_block.t_residuals,
            decoder,
            dt_context,
            stream,
            input.meta.delta_t_max,
        );

        inter_block
    }
}

#[cfg(test)]
mod tests {
    use crate::codec::compressed::adu::interblock::AduInterBlock;
    use crate::codec::compressed::adu::intrablock::gen_random_intra_block;
    use crate::codec::compressed::adu::AduCompression;
    use crate::codec::compressed::stream::CompressedInput;
    use crate::codec::{CodecMetadata, WriteCompression};
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

    fn compress_inter_block() -> Result<(AduInterBlock, Vec<u8>), Box<dyn Error>> {
        let mut encoder = setup_encoder();
        let intra_block = gen_random_intra_block(1234, encoder.meta.delta_t_max, Some(7));
        // For convenience, we'll just use the intra block's generator.
        let inter_block = AduInterBlock {
            shift_loss_param: intra_block.shift_loss_param,
            d_residuals: intra_block.d_residuals,
            t_residuals: intra_block.dt_residuals,
        };

        assert!(inter_block.compress(&mut encoder).is_ok());

        let written_data = encoder.into_writer().unwrap();

        Ok((inter_block, written_data))
    }

    #[test]
    fn test_compress_inter_block() {
        let (_, written_data) = compress_inter_block().unwrap();
        let output_len = written_data.len();
        let input_len = 1028; // Rough approximation
        assert!(output_len < input_len);
        eprintln!("Written data: {:?}", written_data);
    }

    #[test]
    fn test_decompress_inter_block() {
        let (inter_block, written_data) = compress_inter_block().unwrap();
        let tmp_len = written_data.len();

        let mut bufreader = BufReader::new(written_data.as_slice());
        let mut bitreader =
            bitstream_io::BitReader::endian(&mut bufreader, bitstream_io::BigEndian);

        let mut decoder = CompressedInput::new(100, 100);

        let decoded_inter_block = AduInterBlock::decompress(&mut bitreader, &mut decoder);

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
        assert_eq!(
            inter_block.shift_loss_param,
            decoded_inter_block.shift_loss_param
        );
        assert_eq!(inter_block.d_residuals, decoded_inter_block.d_residuals);
        assert_eq!(inter_block.t_residuals, decoded_inter_block.t_residuals);
    }
}
