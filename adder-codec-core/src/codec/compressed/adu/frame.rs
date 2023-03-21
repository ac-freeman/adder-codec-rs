//! An independtly decodable unit of video data.
//!
//! I try to lay out the struct here to be a pretty direct translation of the
//! compressed representation. That is, all the data in the struct is what you get when you
//! decompress an ADU.

use crate::codec::compressed::adu::cube::AduCube;
use crate::codec::compressed::adu::AduCompression;
use crate::codec::compressed::blocks::{DResidual, BLOCK_SIZE_AREA};
use crate::codec::compressed::stream::{CompressedInput, CompressedOutput};
use crate::codec::CodecError;
use crate::codec_old::compressed::compression::Contexts;
use crate::codec_old::compressed::fenwick::context_switching::FenwickModel;
use crate::{AbsoluteT, DeltaT, D};
use arithmetic_coding::Encoder;
use bitstream_io::{BigEndian, BitRead, BitReader, BitWrite, BitWriter};
use std::io::{Error, Read, Write};
use std::mem;

#[derive(Clone)]
pub struct AduChannel {
    /// The number of cubes in the ADU.
    num_cubes: u16,

    /// The cubes in the ADU.
    pub(crate) cubes: Vec<AduCube>,
}

impl AduCompression for AduChannel {
    fn compress<W: Write>(
        &self,
        encoder: &mut Encoder<FenwickModel, BitWriter<W, BigEndian>>,
        contexts: &mut Contexts,
        stream: &mut BitWriter<W, BigEndian>,
        dtm: DeltaT,
    ) -> Result<(), CodecError> {
        // Get the context references
        let mut u8_context = contexts.u8_general_context;

        encoder.model.set_context(u8_context);

        // Write the number of cubes
        for byte in self.num_cubes.to_be_bytes().iter() {
            encoder.encode(Some(&(*byte as usize)), stream)?;
        }

        println!("num_cubes: {}", self.num_cubes);
        debug_assert_eq!(self.num_cubes, 240);

        // Write the cubes
        for cube in self.cubes.iter() {
            // if cube.idx_y == 11 && cube.idx_x == 19 {
            //     dbg!(cube.inter_blocks.last().unwrap());
            // }
            cube.compress(encoder, contexts, stream, dtm)?;
        }

        Ok(())
    }

    fn decompress<R: Read>(
        stream: &mut BitReader<R, BigEndian>,
        input: &mut CompressedInput<R>,
    ) -> Self {
        // Get the context references
        let mut decoder = input.arithmetic_coder.as_mut().unwrap();
        let mut u8_context = input.contexts.as_mut().unwrap().u8_general_context;

        decoder.model.set_context(u8_context);

        // Read the number of cubes
        let mut bytes = [0; 2];
        for byte in bytes.iter_mut() {
            *byte = decoder.decode(stream).unwrap().unwrap() as u8;
        }
        let num_cubes = u16::from_be_bytes(bytes);
        debug_assert_eq!(num_cubes, 240);

        println!("num_cubes: {}", num_cubes);

        // Read the cubes
        let mut cubes = Vec::new();
        for _ in 0..num_cubes {
            cubes.push(AduCube::decompress(stream, input));
        }
        // dbg!(cubes.last().unwrap().inter_blocks.last().unwrap());

        Self { num_cubes, cubes }
    }

    fn decompress_debug<R: Read>(
        stream: &mut BitReader<R, BigEndian>,
        input: &mut CompressedInput<R>,
        reference_adu: &Adu,
    ) -> Self {
        todo!()
    }
}

/// A whole spatial frame of data
#[derive(Clone)]
pub struct Adu {
    /// The timestamp of the first event in the ADU.
    pub(crate) head_event_t: AbsoluteT,

    pub(crate) cubes_r: AduChannel,
    pub(crate) cubes_g: AduChannel,
    pub(crate) cubes_b: AduChannel,
}

pub enum AduChannelType {
    R,
    G,
    B,
}

impl Adu {
    pub fn new() -> Self {
        Self {
            head_event_t: 0,
            cubes_r: AduChannel {
                num_cubes: 0,
                cubes: Vec::new(),
            },
            cubes_g: AduChannel {
                num_cubes: 0,
                cubes: Vec::new(),
            },
            cubes_b: AduChannel {
                num_cubes: 0,
                cubes: Vec::new(),
            },
        }
    }

    pub fn add_cube(&mut self, cube: AduCube, channel: AduChannelType) {
        match channel {
            AduChannelType::R => {
                self.cubes_r.cubes.push(cube);
                self.cubes_r.num_cubes += 1;
            }
            AduChannelType::G => {
                self.cubes_g.cubes.push(cube);
                self.cubes_g.num_cubes += 1;
            }
            AduChannelType::B => {
                self.cubes_b.cubes.push(cube);
                self.cubes_b.num_cubes += 1;
            }
        }
    }
}

impl AduCompression for Adu {
    fn compress<W: Write>(
        &self,
        encoder: &mut Encoder<FenwickModel, BitWriter<W, BigEndian>>,
        contexts: &mut Contexts,
        stream: &mut BitWriter<W, BigEndian>,
        dtm: DeltaT,
    ) -> Result<(), CodecError> {
        // Get the context references
        let mut u8_context = contexts.u8_general_context;

        encoder.model.set_context(u8_context);

        // Write the head event timestamp
        for byte in self.head_event_t.to_be_bytes().iter() {
            encoder.encode(Some(&(*byte as usize)), stream)?;
        }

        // Write the cubes
        self.cubes_r.compress(encoder, contexts, stream, dtm)?;
        // self.cubes_g.compress(encoder, contexts, stream, dtm)?;
        // self.cubes_b.compress(encoder, contexts, stream, dtm)?;

        encoder.model.set_context(contexts.eof_context);
        encoder.encode(None, stream)?;
        encoder.flush(stream).unwrap();
        stream.byte_align()?;
        stream.flush()?;

        Ok(())
    }

    fn decompress<R: Read>(
        stream: &mut BitReader<R, BigEndian>,
        input: &mut CompressedInput<R>,
    ) -> Self {
        // Get the context references
        let mut decoder = input.arithmetic_coder.as_mut().unwrap();
        let mut u8_context = input.contexts.as_mut().unwrap().u8_general_context;
        let mut eof_context = input.contexts.as_mut().unwrap().eof_context;

        decoder.model.set_context(u8_context);

        // Read the head event timestamp
        let mut bytes = [0; mem::size_of::<AbsoluteT>()];
        for byte in bytes.iter_mut() {
            *byte = decoder.decode(stream).unwrap().unwrap() as u8;
        }
        let head_event_t = AbsoluteT::from_be_bytes(bytes);

        // Read the cubes
        let cubes_r = AduChannel::decompress(stream, input);
        // let cubes_g = AduChannel::decompress(stream, input);
        // let cubes_b = AduChannel::decompress(stream, input);

        let cubes_g = AduChannel {
            num_cubes: 0,
            cubes: vec![],
        };
        let cubes_b = AduChannel {
            num_cubes: 0,
            cubes: vec![],
        };

        let mut decoder = input.arithmetic_coder.as_mut().unwrap();
        decoder.model.set_context(eof_context);
        assert!(decoder.decode(stream).unwrap().is_none());
        // stream.byte_align();

        Self {
            head_event_t,
            cubes_r,
            cubes_g,
            cubes_b,
        }
    }

    fn decompress_debug<R: Read>(
        stream: &mut BitReader<R, BigEndian>,
        input: &mut CompressedInput<R>,
        reference_adu: &Adu,
    ) -> Self {
        // Get the context references
        let mut decoder = input.arithmetic_coder.as_mut().unwrap();
        let mut u8_context = input.contexts.as_mut().unwrap().u8_general_context;
        let mut eof_context = input.contexts.as_mut().unwrap().eof_context;

        decoder.model.set_context(u8_context);

        // Read the head event timestamp
        let mut bytes = [0; mem::size_of::<AbsoluteT>()];
        for byte in bytes.iter_mut() {
            *byte = decoder.decode(stream).unwrap().unwrap() as u8;
        }
        let head_event_t = AbsoluteT::from_be_bytes(bytes);

        assert_eq!(head_event_t, reference_adu.head_event_t);

        // Read the cubes
        let cubes_r = AduChannel::decompress(stream, input);

        for (cube, reference_cube) in cubes_r.cubes.iter().zip(reference_adu.cubes_r.cubes.iter()) {
            assert_eq!(cube.idx_y, reference_cube.idx_y);
            assert_eq!(cube.idx_x, reference_cube.idx_x);
            assert_eq!(cube.intra_block, reference_cube.intra_block);
            assert_eq!(cube.num_inter_blocks, reference_cube.num_inter_blocks);
            for ((idx, inter_block), reference_inter_block) in cube
                .inter_blocks
                .iter()
                .enumerate()
                .zip(reference_cube.inter_blocks.iter())
            {
                assert_eq!(
                    inter_block.shift_loss_param,
                    reference_inter_block.shift_loss_param
                );

                for px_idx in 0..BLOCK_SIZE_AREA {
                    let d = inter_block.d_residuals[px_idx];
                    let d_ref = reference_inter_block.d_residuals[px_idx];
                    assert_eq!(d, d_ref);

                    let t = inter_block.t_residuals[px_idx];
                    let t_ref = reference_inter_block.t_residuals[px_idx];
                    assert_eq!(t, t_ref);
                }
            }
        }
        // let cubes_g = AduChannel::decompress(stream, input);
        // let cubes_b = AduChannel::decompress(stream, input);

        let cubes_g = AduChannel {
            num_cubes: 0,
            cubes: vec![],
        };
        let cubes_b = AduChannel {
            num_cubes: 0,
            cubes: vec![],
        };

        let mut decoder = input.arithmetic_coder.as_mut().unwrap();
        decoder.model.set_context(eof_context);
        assert!(decoder.decode(stream).unwrap().is_none());

        stream.read_bit().unwrap();
        stream.byte_align();

        Self {
            head_event_t,
            cubes_r,
            cubes_g,
            cubes_b,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::codec::compressed::adu::cube::AduCube;
    use crate::codec::compressed::adu::frame::{compare_channels, Adu, AduChannel};
    use crate::codec::compressed::adu::interblock::AduInterBlock;
    use crate::codec::compressed::adu::intrablock::gen_random_intra_block;
    use crate::codec::compressed::adu::AduCompression;
    use crate::codec::compressed::stream::{CompressedInput, CompressedOutput};
    use crate::codec::decoder::Decoder;
    use crate::codec::{CodecMetadata, WriteCompression};
    use crate::codec_old::compressed::fenwick::context_switching::FenwickModel;
    use arithmetic_coding::Encoder;
    use bitstream_io::{BigEndian, BitRead, BitReader, BitWrite};
    use rand::prelude::StdRng;
    use rand::{Rng, SeedableRng};
    use std::cmp::min;
    use std::error::Error;
    use std::io::{BufReader, Cursor};

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

    fn gen_rand_channel(
        encoder: &mut CompressedOutput<Vec<u8>>,
        seed: Option<u64>,
        mut rng: StdRng,
    ) -> AduChannel {
        let mut cubes = Vec::new();
        for _ in 0..10 {
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
            cubes.push(cube);
        }

        let mut channel = AduChannel {
            num_cubes: cubes.len() as u16,
            cubes,
        };
        channel
    }

    fn setup_channel(encoder: &mut CompressedOutput<Vec<u8>>, seed: Option<u64>) -> AduChannel {
        let mut rng = match seed {
            None => StdRng::from_rng(rand::thread_rng()).unwrap(),
            Some(num) => StdRng::seed_from_u64(num),
        };

        gen_rand_channel(encoder, seed, rng)
    }

    fn compress_channel() -> Result<(AduChannel, Vec<u8>), Box<dyn Error>> {
        let mut encoder = setup_encoder();
        let channel = setup_channel(&mut encoder, Some(7));

        assert!(channel
            .compress(
                encoder.arithmetic_coder.as_mut().unwrap(),
                encoder.contexts.as_mut().unwrap(),
                encoder.stream.as_mut().unwrap(),
                encoder.meta.delta_t_max
            )
            .is_ok());

        let written_data = encoder.into_writer().unwrap();

        Ok((channel, written_data))
    }

    fn setup_adu(encoder: &mut CompressedOutput<Vec<u8>>, seed: Option<u64>) -> Adu {
        let mut rng = match seed {
            None => StdRng::from_rng(rand::thread_rng()).unwrap(),
            Some(num) => StdRng::seed_from_u64(num),
        };

        let cubes_r = gen_rand_channel(encoder, seed, rng.clone());
        let cubes_g = gen_rand_channel(encoder, seed, rng.clone());
        let cubes_b = gen_rand_channel(encoder, seed, rng.clone());

        Adu {
            head_event_t: rng.gen(),
            cubes_r,
            cubes_g,
            cubes_b,
        }
    }

    fn compress_adu() -> Result<(Adu, Vec<u8>), Box<dyn Error>> {
        let mut encoder = setup_encoder();

        let adu = setup_adu(&mut encoder, Some(7));

        assert!(adu
            .compress(
                encoder.arithmetic_coder.as_mut().unwrap(),
                encoder.contexts.as_mut().unwrap(),
                encoder.stream.as_mut().unwrap(),
                encoder.meta.delta_t_max
            )
            .is_ok());

        let written_data = encoder.into_writer().unwrap();

        Ok((adu, written_data))
    }

    #[test]
    fn test_compress_channel() {
        let (_, written_data) = compress_channel().unwrap();
        let output_len = written_data.len();
        let input_len = 1028 * 11 * 10; // Rough approximation
        assert!(output_len < input_len);
        eprintln!("Output length: {}", output_len);
        eprintln!("Input length: {}", input_len);
    }

    #[test]
    fn test_decompress_channel() {
        let (channel, written_data) = compress_channel().unwrap();
        let tmp_len = written_data.len();

        let mut bufreader = BufReader::new(written_data.as_slice());
        let mut bitreader =
            bitstream_io::BitReader::endian(&mut bufreader, bitstream_io::BigEndian);

        let mut decoder = CompressedInput::new(100, 100);

        let decoded_channel = AduChannel::decompress(&mut bitreader, &mut decoder);

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
        compare_channels(&channel, &decoded_channel);
    }

    #[test]
    fn test_compress_adu() {
        let (_, written_data) = compress_adu().unwrap();
        let output_len = written_data.len();
        let input_len = 1028 * 11 * 10 * 3; // Rough approximation
        assert!(output_len < input_len);
        eprintln!("Output length: {}", output_len);
        eprintln!("Input length: {}", input_len);
    }

    #[test]
    fn test_decompress_adu() {
        let (adu, written_data) = compress_adu().unwrap();
        let tmp_len = written_data.len();

        let mut bufreader = BufReader::new(written_data.as_slice());
        let mut bitreader =
            bitstream_io::BitReader::endian(&mut bufreader, bitstream_io::BigEndian);

        let mut decoder = CompressedInput::new(100, 100);

        let decoded_adu = Adu::decompress(&mut bitreader, &mut decoder);

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
        assert_eq!(adu.head_event_t, decoded_adu.head_event_t);

        compare_channels(&adu.cubes_r, &decoded_adu.cubes_r);
        compare_channels(&adu.cubes_g, &decoded_adu.cubes_g);
        compare_channels(&adu.cubes_b, &decoded_adu.cubes_b);
    }

    #[test]
    fn test_chained_streams_eof() {
        let bufwriter = Vec::new();
        let mut output = CompressedOutput::new(
            CodecMetadata {
                codec_version: 0,
                header_size: 0,
                time_mode: Default::default(),
                plane: Default::default(),
                tps: 0,
                ref_interval: 255,
                delta_t_max: 10200,
                event_size: 0,
                source_camera: Default::default(),
            },
            bufwriter,
        );

        let dtm = output.meta.delta_t_max;
        let ref_interval = output.meta.ref_interval;

        let num1: i32 = 123456789;
        let num2: i32 = 987654321;
        let mut stream = output.stream.as_mut().unwrap();
        let mut encoder = output.arithmetic_coder.as_mut().unwrap();
        {
            let mut u8_context = output.contexts.as_mut().unwrap().u8_general_context;
            let mut eof_context = output.contexts.as_mut().unwrap().eof_context;
            // encode the data
            encoder.model.set_context(u8_context);

            for byte in num1.to_be_bytes().iter() {
                encoder.encode(Some(&(*byte as usize)), stream).unwrap();
            }
            encoder.encode(Some(&255), stream).unwrap();

            {
                // flush the data
                encoder.flush(stream).unwrap();
                stream.byte_align();
            }
            // Write a raw byte, for testing
            for _ in 0..8 {
                stream.write_bytes(&[255]).unwrap();
            }
            // Junk symbol, to test end of segment
            stream.write_bytes(&[0]).unwrap();

            // Reset the arithmetic encoder
            let mut source_model =
                FenwickModel::with_symbols(min(dtm as usize * 2, u16::MAX as usize), 1 << 30);
            *encoder = Encoder::new(source_model);

            for byte in num2.to_be_bytes().iter() {
                encoder.encode(Some(&(*byte as usize)), stream).unwrap();
            }
            encoder.encode(Some(&255), stream).unwrap()
            // encoder.encode(None, stream).unwrap();
        }

        {
            // flush the data
            encoder.flush(stream).unwrap();
            stream.flush().unwrap();
        }

        let mut written_data = output.into_writer().unwrap();
        let output_len = written_data.len();

        let mut bufreader = BufReader::new(Cursor::new(written_data));
        let mut bitreader = BitReader::endian(bufreader, BigEndian);
        let mut input: CompressedInput<BufReader<Cursor<Vec<u8>>>> =
            CompressedInput::new(dtm, ref_interval);

        {
            // Decode the data
            let mut decoder = input.arithmetic_coder.as_mut().unwrap();
            let mut u8_context = input.contexts.as_mut().unwrap().u8_general_context;

            decoder.model.set_context(u8_context);

            let mut bytes = [0; 4];
            for byte in bytes.iter_mut() {
                *byte = decoder.decode(&mut bitreader).unwrap().unwrap() as u8;
            }
            let sym1 = i32::from_be_bytes(bytes);
            assert_eq!(sym1, num1);

            // assert!(decoder.decode(&mut bitreader).unwrap().is_none());
            assert_eq!(decoder.decode(&mut bitreader).unwrap(), Some(255));

            // loop {
            //     eprintln!("{}", bitreader.read_bit().unwrap());
            // }

            // loop {
            //     eprintln!("{}", bitreader.read_bit().unwrap());
            // }
            bitreader.byte_align();
            while bitreader.read_to_vec(1).unwrap() == vec![255] {}
            // let next_byte = bitreader.read_to_vec(1).unwrap();
            // assert_eq!(next_byte, vec![255]);

            // Reset the arithmetic decoder
            let mut source_model =
                FenwickModel::with_symbols(min(dtm as usize * 2, u16::MAX as usize), 1 << 30);
            *decoder = arithmetic_coding::Decoder::new(source_model);

            // assert_eq!(next_byte, vec![255]);
            let mut bytes = [0; 4];
            for byte in bytes.iter_mut() {
                *byte = decoder.decode(&mut bitreader).unwrap().unwrap() as u8;
            }
            let sym2 = i32::from_be_bytes(bytes);
            assert_eq!(sym2, num2);

            // assert!(decoder.decode(&mut bitreader).unwrap().is_none());
            assert_eq!(decoder.decode(&mut bitreader).unwrap(), Some(255));
        }
    }
}

/// Helper function for test code
pub fn compare_channels(channel: &AduChannel, decoded_channel: &AduChannel) {
    assert_eq!(channel.num_cubes, decoded_channel.num_cubes);

    for (cube, decoded_cube) in channel.cubes.iter().zip(decoded_channel.cubes.iter()) {
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
        for (block, decoded_block) in cube.inter_blocks.iter().zip(&decoded_cube.inter_blocks) {
            assert_eq!(block.shift_loss_param, decoded_block.shift_loss_param);
            assert_eq!(block.d_residuals, decoded_block.d_residuals);
            assert_eq!(block.t_residuals, decoded_block.t_residuals);
        }
    }
}
