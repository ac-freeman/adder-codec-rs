//! An independtly decodable unit of video data.
//!
//! I try to lay out the struct here to be a pretty direct translation of the
//! compressed representation. That is, all the data in the struct is what you get when you
//! decompress an ADU.

use crate::codec::compressed::adu::cube::AduCube;
use crate::codec::compressed::adu::{AduComponentCompression, AduCompression};
use crate::codec::compressed::blocks::prediction::Contexts;
use crate::codec::compressed::blocks::{DResidual, BLOCK_SIZE_AREA};
use crate::codec::compressed::fenwick::context_switching::FenwickModel;
use crate::codec::compressed::stream::{CompressedInput, CompressedOutput};
use crate::codec::{CodecError, CodecMetadata};
use crate::{AbsoluteT, DeltaT, D};
use arithmetic_coding::{Decoder, Encoder};
use bitstream_io::{BigEndian, BitRead, BitReader, BitWrite, BitWriter};
use std::cmp::min;
use std::io::{BufWriter, Cursor, Error, Read, Write};
use std::mem;

#[derive(Clone)]
pub struct AduChannel {
    /// The number of cubes in the ADU.
    num_cubes: u16,

    /// The cubes in the ADU.
    pub(crate) cubes: Vec<AduCube>,
}

impl AduComponentCompression for AduChannel {
    fn compress(
        &self,
        encoder: &mut Encoder<FenwickModel, BitWriter<Vec<u8>, BigEndian>>,
        contexts: &mut Contexts,
        stream: &mut BitWriter<Vec<u8>, BigEndian>,
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

        // Write the cubes
        for cube in self.cubes.iter() {
            // if cube.idx_y == 11 && cube.idx_x == 19 {
            //     dbg!(cube.inter_blocks.last().unwrap());
            // }
            cube.compress(encoder, contexts, stream, dtm)?;
        }

        Ok(())
    }

    fn decompress(
        decoder: &mut Decoder<FenwickModel, BitReader<Cursor<Vec<u8>>, BigEndian>>,
        contexts: &mut Contexts,
        stream: &mut BitReader<Cursor<Vec<u8>>, BigEndian>,
        dtm: DeltaT,
    ) -> Self {
        decoder.model.set_context(contexts.u8_general_context);

        // Read the number of cubes
        let mut bytes = [0; 2];
        for byte in bytes.iter_mut() {
            *byte = decoder.decode(stream).unwrap().unwrap() as u8;
        }
        let num_cubes = u16::from_be_bytes(bytes);

        println!("num_cubes: {}", num_cubes);

        // Read the cubes
        let mut cubes = Vec::new();
        for _ in 0..num_cubes {
            cubes.push(AduCube::decompress(decoder, contexts, stream, dtm));
        }
        // dbg!(cubes.last().unwrap().inter_blocks.last().unwrap());

        Self { num_cubes, cubes }
    }
}

/// A whole spatial frame of data
#[derive(Clone)]
pub struct Adu {
    /// The number of bytes in the compressed ADU. This number is not compressed.
    pub(crate) num_bytes: u64,

    /// The timestamp of the first event in the ADU. This number is not compressed.
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
            num_bytes: 0,
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

    // Resets the necessary parameters for a new ADU, but keeps the existing log of timestamp
    // memory and the existing cubes.
    // pub(crate) fn reset_new(&mut self) {
    //     self.cubes_r.num_cubes = 0;
    //     self.cubes_g.num_cubes = 0;
    //     self.cubes_b.num_cubes = 0;
    //
    // }

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

    /// Decompress just the header (the number of bytes and the head event timestamp) of the ADU.
    ///
    /// Useful for scrubbing a stream without having to decode an entire ADU.
    pub fn decompress_header<R: Read>(stream: &mut BitReader<R, BigEndian>) -> (u64, AbsoluteT) {
        let mut buffer = [0u8; 8];
        stream.read_bytes(&mut buffer).unwrap();
        let num_bytes = u64::from_be_bytes(buffer);

        // Decode the head_event_t
        let mut buffer = [0u8; 4];
        stream.read_bytes(&mut buffer).unwrap();
        let head_event_t = u32::from_be_bytes(buffer);
        (num_bytes, head_event_t)
    }
}

impl AduCompression for Adu {
    fn compress<W: Write>(
        &self,
        encoder: &mut Encoder<FenwickModel, BitWriter<Vec<u8>, BigEndian>>,
        contexts: &mut Contexts,
        stream: &mut BitWriter<W, BigEndian>,
        dtm: DeltaT,
        ref_interval: DeltaT,
    ) -> Result<(), CodecError> {
        let mut source_model =
            FenwickModel::with_symbols(min(dtm as usize * 2, u16::MAX as usize), 1 << 30);

        *contexts = Contexts::new(
            &mut source_model,
            CodecMetadata {
                codec_version: 0,
                header_size: 0,
                time_mode: Default::default(),
                plane: Default::default(),
                tps: 0,
                ref_interval,
                delta_t_max: dtm,
                event_size: 0,
                source_camera: Default::default(),
            },
        );

        *encoder = Encoder::new(source_model);

        // Get the context references
        // let mut u8_context = contexts.u8_general_context;
        //
        // encoder.model.set_context(u8_context);
        //
        // // Write the head event timestamp
        // for byte in self.head_event_t.to_be_bytes().iter() {
        //     encoder.encode(Some(&(*byte as usize)), stream)?;
        // }

        // Create a temporary u8 stream to write the arithmetic-coded data to
        let mut temp_stream = BitWriter::endian(Vec::new(), BigEndian);

        // Write the cubes
        self.cubes_r
            .compress(encoder, contexts, &mut temp_stream, dtm)?;
        self.cubes_g
            .compress(encoder, contexts, &mut temp_stream, dtm)?;
        self.cubes_b
            .compress(encoder, contexts, &mut temp_stream, dtm)?;

        encoder.model.set_context(contexts.eof_context);
        encoder.encode(None, &mut temp_stream)?;
        encoder.flush(&mut temp_stream).unwrap();
        temp_stream.byte_align()?;
        temp_stream.flush()?;

        // Get the number of bytes written to the temporary stream
        let written_data = temp_stream.into_writer();
        let num_bytes = written_data.len() as u64;

        // Write the number of bytes to the stream
        stream.write_bytes(&num_bytes.to_be_bytes())?;

        // Write the head event timestamp to the stream
        stream.write_bytes(&self.head_event_t.to_be_bytes())?;

        // Write the temporary stream to the actual stream
        stream.write_bytes(&written_data)?;

        Ok(())
    }

    fn decompress<R: Read>(
        decoder: &mut Decoder<FenwickModel, BitReader<Cursor<Vec<u8>>, BigEndian>>,
        contexts: &mut Contexts,
        stream: &mut BitReader<R, BigEndian>,
        dtm: DeltaT,
        ref_interval: DeltaT,
    ) -> Self {
        let mut source_model =
            FenwickModel::with_symbols(min(dtm as usize * 2, u16::MAX as usize), 1 << 30);

        *contexts = Contexts::new(
            &mut source_model,
            CodecMetadata {
                codec_version: 0,
                header_size: 0,
                time_mode: Default::default(),
                plane: Default::default(),
                tps: 0,
                ref_interval,
                delta_t_max: dtm,
                event_size: 0,
                source_camera: Default::default(),
            },
        );

        *decoder = Decoder::new(source_model);

        let (num_bytes, head_event_t) = Self::decompress_header(stream);
        // Take a slice of the next num_bytes bytes from the stream
        let mut adu_bytes = stream.read_to_vec(num_bytes as usize).unwrap();
        let mut adu_stream = BitReader::endian(Cursor::new(adu_bytes), BigEndian);

        // Read the cubes
        let cubes_r = AduChannel::decompress(decoder, contexts, &mut adu_stream, dtm);
        let cubes_g = AduChannel::decompress(decoder, contexts, &mut adu_stream, dtm);
        let cubes_b = AduChannel::decompress(decoder, contexts, &mut adu_stream, dtm);

        decoder.model.set_context(contexts.eof_context);
        assert!(decoder.decode(&mut adu_stream).unwrap().is_none());
        // stream.byte_align();

        Self {
            num_bytes,
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
    use crate::codec::compressed::adu::{add_eof, AduComponentCompression, AduCompression};
    use crate::codec::compressed::fenwick::context_switching::FenwickModel;
    use crate::codec::compressed::stream::{CompressedInput, CompressedOutput};
    use crate::codec::decoder::Decoder;
    use crate::codec::{CodecMetadata, WriteCompression};
    use arithmetic_coding::Encoder;
    use bitstream_io::{BigEndian, BitRead, BitReader, BitWrite, BitWriter};
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

        add_eof(&mut encoder);

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
            num_bytes: 0,
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
                encoder.meta.delta_t_max,
                encoder.meta.ref_interval
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

        let mut bufreader = Cursor::new(written_data);
        let mut bitreader = bitstream_io::BitReader::endian(bufreader, bitstream_io::BigEndian);

        let mut compressed_input: CompressedInput<Cursor<Vec<u8>>> = CompressedInput::new(100, 100);
        let mut decoder = compressed_input.arithmetic_coder.as_mut().unwrap();
        let mut contexts = compressed_input.contexts.as_mut().unwrap();

        let decoded_channel =
            AduChannel::decompress(&mut decoder, &mut contexts, &mut bitreader, 100);

        compressed_input
            .arithmetic_coder
            .as_mut()
            .unwrap()
            .model
            .set_context(compressed_input.contexts.as_mut().unwrap().eof_context);
        let eof = compressed_input
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
        let (adu, written_data) = compress_adu().unwrap();
        let output_len = written_data.len();
        let input_len = 1028 * 11 * 10 * 3; // Rough approximation
        assert!(output_len < input_len);
        eprintln!("Output length: {}", output_len);
        eprintln!("Input length: {}", input_len);

        // Decode the num_bytes
        let mut bufreader = BufReader::new(written_data.as_slice());
        let mut bitreader =
            bitstream_io::BitReader::endian(&mut bufreader, bitstream_io::BigEndian);
        let mut buffer = [0u8; 8];
        bitreader.read_bytes(&mut buffer).unwrap();
        let num_bytes = u64::from_be_bytes(buffer);

        assert_eq!(num_bytes, output_len as u64 - 8 - 4);

        // Decode the head_event_t
        let mut buffer = [0u8; 4];
        bitreader.read_bytes(&mut buffer).unwrap();
        let head_event_t = u32::from_be_bytes(buffer);

        assert_eq!(head_event_t, adu.head_event_t);
    }

    #[test]
    fn test_decompress_adu() {
        let (adu, written_data) = compress_adu().unwrap();
        let output_len = written_data.len();

        let mut bufreader = BufReader::new(written_data.as_slice());
        let mut bitreader =
            bitstream_io::BitReader::endian(&mut bufreader, bitstream_io::BigEndian);

        let mut compressed_input: CompressedInput<Cursor<Vec<u8>>> = CompressedInput::new(100, 100);
        let mut decoder = compressed_input.arithmetic_coder.as_mut().unwrap();
        let mut contexts = compressed_input.contexts.as_mut().unwrap();

        let decoded_adu = Adu::decompress(&mut decoder, &mut contexts, &mut bitreader, 100, 100);

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

        let mut bufreader = Cursor::new(written_data);
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

    #[test]
    fn test_chained_adus() {
        let mut encoder = setup_encoder();

        let adu1 = setup_adu(&mut encoder, Some(7));

        assert!(adu1
            .compress(
                encoder.arithmetic_coder.as_mut().unwrap(),
                encoder.contexts.as_mut().unwrap(),
                encoder.stream.as_mut().unwrap(),
                encoder.meta.delta_t_max,
                encoder.meta.ref_interval
            )
            .is_ok());

        let adu2 = setup_adu(&mut encoder, Some(8));

        assert!(adu2
            .compress(
                encoder.arithmetic_coder.as_mut().unwrap(),
                encoder.contexts.as_mut().unwrap(),
                encoder.stream.as_mut().unwrap(),
                encoder.meta.delta_t_max,
                encoder.meta.ref_interval
            )
            .is_ok());

        let adu3 = setup_adu(&mut encoder, Some(18));

        assert!(adu3
            .compress(
                encoder.arithmetic_coder.as_mut().unwrap(),
                encoder.contexts.as_mut().unwrap(),
                encoder.stream.as_mut().unwrap(),
                encoder.meta.delta_t_max,
                encoder.meta.ref_interval
            )
            .is_ok());

        // The `stream` field of the encoder is now has our two ADUs in it.
        let written_data = encoder.into_writer().unwrap();

        // Now we can decode them.
        let mut bufreader = BufReader::new(written_data.as_slice());
        let mut bitreader =
            bitstream_io::BitReader::endian(&mut bufreader, bitstream_io::BigEndian);

        let mut compressed_input: CompressedInput<Cursor<Vec<u8>>> = CompressedInput::new(100, 100);
        let mut decoder = compressed_input.arithmetic_coder.as_mut().unwrap();
        let mut contexts = compressed_input.contexts.as_mut().unwrap();

        let decoded_adu = Adu::decompress(&mut decoder, &mut contexts, &mut bitreader, 100, 100);

        assert_eq!(adu1.head_event_t, decoded_adu.head_event_t);

        compare_channels(&adu1.cubes_r, &decoded_adu.cubes_r);
        compare_channels(&adu1.cubes_g, &decoded_adu.cubes_g);
        compare_channels(&adu1.cubes_b, &decoded_adu.cubes_b);

        let decoded_adu = Adu::decompress(&mut decoder, &mut contexts, &mut bitreader, 100, 100);

        assert_eq!(adu2.head_event_t, decoded_adu.head_event_t);

        compare_channels(&adu2.cubes_r, &decoded_adu.cubes_r);
        compare_channels(&adu2.cubes_g, &decoded_adu.cubes_g);
        compare_channels(&adu2.cubes_b, &decoded_adu.cubes_b);

        let decoded_adu = Adu::decompress(&mut decoder, &mut contexts, &mut bitreader, 100, 100);

        assert_eq!(adu3.head_event_t, decoded_adu.head_event_t);

        compare_channels(&adu3.cubes_r, &decoded_adu.cubes_r);
        compare_channels(&adu3.cubes_g, &decoded_adu.cubes_g);
        compare_channels(&adu3.cubes_b, &decoded_adu.cubes_b);
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
