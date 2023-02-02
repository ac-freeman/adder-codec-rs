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
use crate::framer::driver::EventCoordless;
use crate::{DeltaT, Event, TimeMode, D};

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
    type Symbol = DResidual;
    type ValueError = ValueError;
    type B = u64;

    fn probability(
        &self,
        symbol: Option<&Self::Symbol>,
    ) -> Result<Range<Self::B>, Self::ValueError> {
        let fenwick_symbol = match symbol {
            Some(c) if *c >= -255 && *c <= 255 => Some((*c + 255) as usize),
            _ => None,
        };
        self.fenwick_model.probability(fenwick_symbol.as_ref())
    }

    fn denominator(&self) -> Self::B {
        self.fenwick_model.denominator()
    }

    fn max_denominator(&self) -> Self::B {
        self.fenwick_model.max_denominator()
    }

    fn symbol(&self, value: Self::B) -> Option<Self::Symbol> {
        let index = self.fenwick_model.symbol(value)?;
        self.alphabet.get(index).copied()
    }

    fn update(&mut self, symbol: Option<&Self::Symbol>) {
        let fenwick_symbol = match symbol {
            Some(c) if *c >= -255 && *c <= 255 => Some((*c + 255) as usize),
            _ => None,
        };
        self.fenwick_model.update(fenwick_symbol.as_ref());
    }
}

#[derive(Clone)]
pub struct BlockDeltaTResidualModel {
    alphabet: Vec<DeltaTResidual>,
    fenwick_model: FenwickModel,
    delta_t_max: i64,
}

pub type DeltaTResidual = i64;

impl BlockDeltaTResidualModel {
    #[must_use]
    pub fn new(delta_t_max: DeltaT) -> Self {
        let alphabet: Vec<DeltaTResidual> = (-(delta_t_max as i64)..delta_t_max as i64).collect();
        let fenwick_model =
            FenwickModel::builder(delta_t_max as usize * 2 + 1, 1 << (delta_t_max.ilog2() + 3))
                .panic_on_saturation()
                .build();
        Self {
            alphabet,
            fenwick_model,
            delta_t_max: delta_t_max.into(),
        }
    }
}

impl Model for BlockDeltaTResidualModel {
    type Symbol = DeltaTResidual;
    type ValueError = ValueError;
    type B = u64;

    fn probability(
        &self,
        symbol: Option<&Self::Symbol>,
    ) -> Result<Range<Self::B>, Self::ValueError> {
        let fenwick_symbol = match symbol {
            Some(c) if *c >= -self.delta_t_max && *c <= self.delta_t_max => {
                Some((*c + self.delta_t_max) as usize)
            }
            _ => None,
        };
        self.fenwick_model.probability(fenwick_symbol.as_ref())
    }

    fn denominator(&self) -> Self::B {
        self.fenwick_model.denominator()
    }

    fn max_denominator(&self) -> Self::B {
        self.fenwick_model.max_denominator()
    }

    fn symbol(&self, value: Self::B) -> Option<Self::Symbol> {
        let index = self.fenwick_model.symbol(value)?;
        self.alphabet.get(index).copied()
    }

    fn update(&mut self, symbol: Option<&Self::Symbol>) {
        let fenwick_symbol = match symbol {
            Some(c) if *c >= -self.delta_t_max && *c <= self.delta_t_max => {
                Some((*c + self.delta_t_max) as usize)
            }
            _ => None,
        };
        self.fenwick_model.update(fenwick_symbol.as_ref());
    }
}

// #[derive(Clone)]
// pub struct BlockEventResidualModel {
//     d_model: BlockDResidualModel,
//     delta_t_model: BlockDeltaTResidualModel,
// }
//
// pub type EventResidual = (DResidual, DeltaTResidual);

// impl BlockEventResidualModel {
//     // type Symbol = EventResidual;
//     // type ValueError = ValueError;
//     // type B = u64;
//
//     #[must_use]
//     pub fn new(delta_t_max: DeltaT) -> Self {
//         let d_model = BlockDResidualModel::new();
//         let delta_t_model = BlockDeltaTResidualModel::new(delta_t_max);
//         Self {
//             d_model,
//             delta_t_model,
//         }
//     }
//
//     pub fn encode_all(
//         &mut self,
//         symbols: impl IntoIterator<Item = EventResidual>,
//     ) -> Result<(), Error> {
//         for symbol in symbols {
//
//             self.encode(Some(&symbol))?;
//         }
//         self.encode(None)?;
//         self.flush()?;
//
//         let mut residuals = Vec::with_capacity(events.len());
//         for event in events {
//             residuals.push(self.encode(event));
//         }
//         residuals
//     }
// }

/// Setup the context-adaptive intra-prediction model for an event block.
/// For now, just do a naive model that only looks at the previous 1 coded event.
/// Note: will have to work differently with delta-t vs absolute-t modes...
/// TODO: encode all the D-vals first, then the dt values?
/// TODO: use a more sophisticated model.
struct BlockIntraPredictionContext {
    prev_coded_event: Option<EventCoordless>,
    prediction_mode: TimeMode,
}

// impl BlockIntraPredictionContext {
//     fn new() -> Self {
//         Self {
//             prev_coded_event: None,
//             prediction_mode: TimeMode::AbsoluteT,
//         }
//     }
//
//     fn encode_block(&mut self, block: &mut Block) {
//
//     }
//
//     }
//
//     /// Encode the prediction residual for an event based on the previous coded event
//     fn encode_event(&mut self, event: &EventCoordless) -> DeltaTResidual {
//         // match self.prediction_mode {
//         //     TimeMode::AbsoluteT => {
//         //         let delta_t = event.t - self.prev_coded_event.unwrap().t;
//         //         delta_t
//         //     }
//         //     TimeMode::DeltaT => {
//         //         let delta_t = event.t - self.prev_coded_event.unwrap().t;
//         //         delta_t
//         //     }
//         //     _ => {}
//         // }
//     }
// }

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
    use crate::codec::compressed::compression::{
        BlockDResidualModel, BlockDeltaTResidualModel, DResidual, DeltaTResidual,
    };
    use arithmetic_coding::{Decoder, Encoder, Model};
    use bitstream_io::{BigEndian, BitReader, BitWrite, BitWriter};
    use rand::Rng;
    use std::io::Read;

    #[test]
    fn test_i16_compression() {
        let model = BlockDResidualModel::new();
        let mut bitwriter = BitWriter::endian(Vec::new(), BigEndian);
        let mut encoder = Encoder::new(model.clone(), &mut bitwriter);

        let input: Vec<DResidual> = vec![
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
        let output: Vec<DResidual> = decoder.decode_all().map(Result::unwrap).collect();
        println!("{:?}", output);
        assert_eq!(output, input);
    }

    #[test]
    fn test_i16_rand_compression() {
        let model = BlockDResidualModel::new();
        let mut bitwriter = BitWriter::endian(Vec::new(), BigEndian);
        let mut encoder = Encoder::new(model.clone(), &mut bitwriter);

        let mut rng = rand::thread_rng();
        let input: Vec<DResidual> = (0..1000).map(|_| rng.gen_range(-255..255)).collect();

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
        let output: Vec<DResidual> = decoder.decode_all().map(Result::unwrap).collect();
        assert_eq!(output, input);
    }

    #[test]
    fn test_delta_t_compression() {
        let model = BlockDeltaTResidualModel::new(255 * 10);
        let mut bitwriter = BitWriter::endian(Vec::new(), BigEndian);
        let mut encoder = Encoder::new(model.clone(), &mut bitwriter);

        let input: Vec<DeltaTResidual> = vec![100, -250, 89, 87, 86, 105, -30, 20, -28, 120];

        let input_len = input.len() * 4;

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
        let output: Vec<DeltaTResidual> = decoder.decode_all().map(Result::unwrap).collect();
        println!("{:?}", output);
        assert_eq!(output, input);
    }

    #[test]
    fn test_delta_t_rand_compression() {
        let delta_t_max = 255 * 10;
        let model = BlockDeltaTResidualModel::new(delta_t_max);
        let mut bitwriter = BitWriter::endian(Vec::new(), BigEndian);
        let mut encoder = Encoder::new(model.clone(), &mut bitwriter);

        let mut rng = rand::thread_rng();
        let input: Vec<DeltaTResidual> = (0..1000)
            .map(|_| rng.gen_range(-(delta_t_max as DeltaTResidual)..delta_t_max as DeltaTResidual))
            .collect();

        let input_len = input.len() * 4;

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
        let output: Vec<DeltaTResidual> = decoder.decode_all().map(Result::unwrap).collect();
        println!("{:?}", output);
        assert_eq!(output, input);
    }
}
