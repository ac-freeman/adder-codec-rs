use crate::codec::compressed::adu::interblock::AduInterBlock;
use crate::codec::compressed::adu::intrablock::AduIntraBlock;
use crate::codec::compressed::adu::AduCompression;
use crate::codec::compressed::stream::{CompressedInput, CompressedOutput};
use crate::codec::{ReadCompression, WriteCompression};
use bitstream_io::{BigEndian, BitReader};
use std::io::{Error, Read, Write};

pub struct AduCube {
    pub(crate) idx_y: u16,

    pub(crate) idx_x: u16,

    pub(crate) intra_block: AduIntraBlock,

    /// The number of inter blocks in the ADU.
    pub(crate) num_inter_blocks: u16,

    /// The inter blocks in the ADU.
    pub(crate) inter_blocks: Vec<AduInterBlock>,
}

impl AduCube {
    pub fn from_intra_block(intra_block: AduIntraBlock, idx_y: u16, idx_x: u16) -> Self {
        Self {
            idx_y,
            idx_x,
            intra_block,
            num_inter_blocks: 0,
            inter_blocks: Vec::new(),
        }
    }

    pub fn add_inter_block(&mut self, inter_block: AduInterBlock) {
        self.num_inter_blocks += 1;
        self.inter_blocks.push(inter_block);
    }
}

impl AduCompression for AduCube {
    fn compress<W: Write>(&self, output: &mut CompressedOutput<W>) -> Result<(), Error> {
        // Get the context references
        let mut encoder = output.arithmetic_coder.as_mut().unwrap();
        let mut u8_context = output.contexts.as_mut().unwrap().u8_general_context;

        encoder.model.set_context(u8_context);

        // Write the cube coordinates
        for byte in self.idx_y.to_be_bytes().iter() {
            encoder.encode(Some(&(*byte as usize)), output.stream.as_mut().unwrap());
        }
        for byte in self.idx_x.to_be_bytes().iter() {
            encoder.encode(Some(&(*byte as usize)), output.stream.as_mut().unwrap());
        }

        // Write the intra block
        self.intra_block.compress(output)?;

        let mut encoder = output.arithmetic_coder.as_mut().unwrap();

        // Write the number of inter blocks
        encoder.model.set_context(u8_context);
        for byte in self.num_inter_blocks.to_be_bytes().iter() {
            encoder.encode(Some(&(*byte as usize)), output.stream.as_mut().unwrap());
        }

        // Write the inter blocks
        for inter_block in &self.inter_blocks {
            inter_block.compress(output)?;
        }

        Ok(())
    }

    fn decompress<R: Read>(
        stream: &mut BitReader<R, BigEndian>,
        input: &mut CompressedInput<R>,
    ) -> Self {
        // Get the context references
        let mut decoder = input.arithmetic_coder.as_mut().unwrap();
        let mut d_context = input.contexts.as_mut().unwrap().d_context;
        let mut dt_context = input.contexts.as_mut().unwrap().dt_context;
        let mut u8_context = input.contexts.as_mut().unwrap().u8_general_context;

        decoder.model.set_context(u8_context);

        // Read the cube coordinates
        let mut bytes = [0; 2];
        for byte in bytes.iter_mut() {
            *byte = decoder.decode(stream).unwrap().unwrap() as u8;
        }
        let idx_y = u16::from_be_bytes(bytes);
        for byte in bytes.iter_mut() {
            *byte = decoder.decode(stream).unwrap().unwrap() as u8;
        }
        let idx_x = u16::from_be_bytes(bytes);

        // Read the intra block
        let intra_block = AduIntraBlock::decompress(stream, input);

        // Initialize empty cube
        let mut cube = Self {
            idx_y,
            idx_x,
            intra_block,
            num_inter_blocks: 0,
            inter_blocks: Vec::new(),
        };

        let mut decoder = input.arithmetic_coder.as_mut().unwrap();

        // Read the number of inter blocks
        let mut bytes = [0; 2];
        decoder.model.set_context(u8_context);
        for byte in bytes.iter_mut() {
            *byte = decoder.decode(stream).unwrap().unwrap() as u8;
        }
        cube.num_inter_blocks = u16::from_be_bytes(bytes);

        // Read the inter blocks
        for _ in 0..cube.num_inter_blocks {
            cube.inter_blocks
                .push(AduInterBlock::decompress(stream, input));
        }

        cube
    }
}

#[cfg(test)]
mod tests {
    use crate::codec::compressed::adu::cube::AduCube;
    use crate::codec::compressed::adu::interblock::AduInterBlock;
    use crate::codec::compressed::adu::intrablock::gen_random_intra_block;
    use crate::codec::compressed::adu::AduCompression;
    use crate::codec::compressed::stream::{CompressedInput, CompressedOutput};
    use crate::codec::{CodecMetadata, WriteCompression};
    use rand::prelude::StdRng;
    use rand::{Rng, SeedableRng};
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

    fn setup_cube(
        encoder: &mut CompressedOutput<Vec<u8>>,
        seed: Option<u64>,
    ) -> crate::codec::compressed::adu::cube::AduCube {
        let mut rng = match seed {
            None => StdRng::from_rng(rand::thread_rng()).unwrap(),
            Some(num) => StdRng::seed_from_u64(num),
        };

        let mut encoder = setup_encoder();
        let intra_block = gen_random_intra_block(1234, encoder.meta.delta_t_max, seed);
        let mut cube = crate::codec::compressed::adu::cube::AduCube::from_intra_block(
            intra_block,
            rng.gen(),
            rng.gen(),
        );
        for _ in 0..10 {
            let intra_block = gen_random_intra_block(1234, encoder.meta.delta_t_max, seed);
            // For convenience, we'll just use the intra block's generator.
            let inter_block = AduInterBlock {
                shift_loss_param: intra_block.shift_loss_param,
                d_residuals: intra_block.d_residuals,
                t_residuals: intra_block.dt_residuals,
            };
            cube.add_inter_block(inter_block);
        }
        cube
    }

    fn compress_cube() -> Result<(AduCube, Vec<u8>), Box<dyn Error>> {
        let mut encoder = setup_encoder();
        let cube = setup_cube(&mut encoder, Some(7));

        assert!(cube.compress(&mut encoder).is_ok());

        let written_data = encoder.into_writer().unwrap();

        Ok((cube, written_data))
    }

    #[test]
    fn test_compress_cube() {
        let (_, written_data) = compress_cube().unwrap();
        let output_len = written_data.len();
        let input_len = 1028 * 11; // Rough approximation
        assert!(output_len < input_len);
        eprintln!("Output length: {}", output_len);
        eprintln!("Input length: {}", input_len);
    }

    #[test]
    fn test_decompress_cube() {
        let (cube, written_data) = compress_cube().unwrap();
        let tmp_len = written_data.len();

        let mut bufreader = BufReader::new(written_data.as_slice());
        let mut bitreader =
            bitstream_io::BitReader::endian(&mut bufreader, bitstream_io::BigEndian);

        let mut decoder = CompressedInput::new(100, 100);

        let decoded_cube = AduCube::decompress(&mut bitreader, &mut decoder);

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
        // assert!(eof.is_none());
        assert_eq!(cube.idx_y, decoded_cube.idx_y);
        assert_eq!(cube.idx_x, decoded_cube.idx_x);
        assert_eq!(
            cube.intra_block.head_event_t,
            decoded_cube.intra_block.head_event_t
        );
        assert_eq!(
            cube.intra_block.head_event_d,
            decoded_cube.intra_block.head_event_d
        );
        assert_eq!(
            cube.intra_block.shift_loss_param,
            decoded_cube.intra_block.shift_loss_param
        );
        assert_eq!(
            cube.intra_block.d_residuals,
            decoded_cube.intra_block.d_residuals
        );
        assert_eq!(
            cube.intra_block.dt_residuals,
            decoded_cube.intra_block.dt_residuals
        );
        assert_eq!(cube.num_inter_blocks, decoded_cube.num_inter_blocks);
        for (block, decoded_block) in cube.inter_blocks.iter().zip(decoded_cube.inter_blocks) {
            assert_eq!(block.shift_loss_param, decoded_block.shift_loss_param);
            assert_eq!(block.d_residuals, decoded_block.d_residuals);
            assert_eq!(block.t_residuals, decoded_block.t_residuals);
        }
    }
}
