use crate::codec::compressed::blocks::Block;
use arithmetic_coding::Model;
use std::ops::Range;

// Intra-coding a block:
// Encode the first D
// Encode the first delta_t
// Get the residual between the first and second D. Encode that
// Get the residual between the first and second delta_t. Encode that
// ... Use zig-zag pattern?

// Inter-coding a block:
// Look at the events in each pixel. Get the residual between the pixel's current D and previous D. Encode that
// Calculate what the EXPECTED delta_t is based on the previous delta_t and the NEW D.
// Get the residual between the pixel's current delta_t and the expected delta_t. Encode that

use crate::codec::compressed::fenwick::{simple::FenwickModel, ValueError};
use crate::D;

#[derive(Clone)]
pub struct BlockDResidualModel {
    alphabet: Vec<DResidual>,
    fenwick_model: FenwickModel,
}

pub type DResidual = i16;

impl BlockDResidualModel {
    #[must_use]
    pub fn new() -> Self {
        let alphabet: Vec<DResidual> = (-255..255).collect();
        let fenwick_model = FenwickModel::builder(u8::MAX as usize * 2 + 1, 1 << 11)
            .panic_on_saturation()
            .build();
        Self {
            alphabet,
            fenwick_model,
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("invalid D: {0}")]
pub struct Error(D);

impl Model for BlockDResidualModel {
    type B = u64;
    type Symbol = DResidual;
    type ValueError = ValueError;

    fn probability(
        &self,
        symbol: Option<&Self::Symbol>,
    ) -> Result<Range<Self::B>, Self::ValueError> {
        let fenwick_symbol = symbol.map(|c| self.alphabet.iter().position(|x| x == c).unwrap());
        self.fenwick_model.probability(fenwick_symbol.as_ref())
    }

    fn symbol(&self, value: Self::B) -> Option<Self::Symbol> {
        let index = self.fenwick_model.symbol(value)?;
        self.alphabet.get(index).copied()
    }

    fn max_denominator(&self) -> Self::B {
        self.fenwick_model.max_denominator()
    }

    fn denominator(&self) -> Self::B {
        self.fenwick_model.denominator()
    }

    fn update(&mut self, symbol: Option<&Self::Symbol>) {
        let fenwick_symbol = match symbol {
            Some(c) if *c >= -255 && *c <= 255 => Some((*c + 255) as usize),
            _ => None,
        };
        self.fenwick_model.update(fenwick_symbol.as_ref());
    }
}

// pub trait Compression {}
// impl Model for Block {
//     type Symbol = ();
//     type ValueError = ();
//     type B = ();
//
//     fn probability(
//         &self,
//         symbol: Option<&Self::Symbol>,
//     ) -> Result<Range<Self::B>, Self::ValueError> {
//         todo!()
//     }
//
//     fn denominator(&self) -> Self::B {
//         todo!()
//     }
//
//     fn max_denominator(&self) -> Self::B {
//         todo!()
//     }
//
//     fn symbol(&self, value: Self::B) -> Option<Self::Symbol> {
//         todo!()
//     }
//
//     fn update(&mut self, _symbol: Option<&Self::Symbol>) {
//         todo!()
//     }
// }

#[cfg(test)]
mod tests {
    use crate::codec::compressed::compression::BlockDResidualModel;
    use arithmetic_coding::{Decoder, Encoder, Model};
    use bitstream_io::{BigEndian, BitReader, BitWrite, BitWriter};
    use rand::Rng;
    use std::io::Read;

    #[test]
    fn test_i16_compression() {
        let model = BlockDResidualModel::new();
        let mut bitwriter = BitWriter::endian(Vec::new(), BigEndian);
        let mut encoder = Encoder::new(model.clone(), &mut bitwriter);

        let input: Vec<i16> = vec![
            0, 1, 2, 3, 4, 5, 6, 7, 8, 8, 8, 1, 2, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9,
        ];

        let input_len = input.len() * 2;

        encoder.encode_all(input.clone()).unwrap();
        bitwriter.byte_align().unwrap();

        let buffer = bitwriter.into_writer();

        let output_len = buffer.len();
        println!("{:?}", &buffer);

        println!("input bytes: {input_len}");
        println!("output bytes: {output_len}");

        println!(
            "compression ratio: {}",
            input_len as f32 / output_len as f32
        );

        let buff: &[u8] = &buffer;
        let bitreader = BitReader::endian(buff, BigEndian);
        let mut decoder = Decoder::new(model, bitreader);
        let output: Vec<i16> = decoder.decode_all().map(Result::unwrap).collect();
        println!("{:?}", output);
        assert_eq!(output, input);
    }

    #[test]
    fn test_i16_rand_compression() {
        let model = BlockDResidualModel::new();
        let mut bitwriter = BitWriter::endian(Vec::new(), BigEndian);
        let mut encoder = Encoder::new(model.clone(), &mut bitwriter);

        let mut rng = rand::thread_rng();
        let input: Vec<i16> = (0..1000).map(|_| rng.gen_range(-255..255)).collect();

        let input_len = input.len() * 2;

        encoder.encode_all(input.clone()).unwrap();
        bitwriter.byte_align().unwrap();

        let buffer = bitwriter.into_writer();

        let output_len = buffer.len();

        println!("input bytes: {input_len}");
        println!("output bytes: {output_len}");

        println!(
            "compression ratio: {}",
            input_len as f32 / output_len as f32
        );

        // Should always be the case, since we can represent any number in [-255, 255] with 9 bits
        assert!(input_len as f32 / output_len as f32 > 1.6);

        let buff: &[u8] = &buffer;
        let bitreader = BitReader::endian(buff, BigEndian);
        let mut decoder = Decoder::new(model, bitreader);
        let output: Vec<i16> = decoder.decode_all().map(Result::unwrap).collect();
        assert_eq!(output, input);
    }
}
