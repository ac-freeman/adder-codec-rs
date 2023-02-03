use arithmetic_coding::{Encoder, Model};
use bitstream_io::{BigEndian, BitWrite, BitWriter};
use std::cmp::{max, min};
use std::fs::File;
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

use crate::codec::compressed::blocks::{Block, ZigZag, ZIGZAG_ORDER};
use crate::codec::compressed::fenwick::{context_switching::FenwickModel, ValueError};
use crate::framer::driver::EventCoordless;
use crate::{DeltaT, TimeMode, D};

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
        let fenwick_model = FenwickModel::with_symbols(u8::MAX as usize * 2 + 1, 1 << 20);
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
        let fenwick_model = FenwickModel::with_symbols(
            delta_t_max as usize * 2 + 1,
            1 << (delta_t_max.ilog2() + 10),
        );
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
struct BlockIntraPredictionContextModel {
    prev_coded_event: Option<EventCoordless>,
    prediction_mode: TimeMode,
    d_model: BlockDResidualModel,
    delta_t_model: BlockDeltaTResidualModel,
    // d_encoder: Option<Encoder<'a, BlockDResidualModel, BitWriter<Vec<u8>, BigEndian>>>,
    // d_writer: BitWriter<Vec<u8>, BigEndian>,
}

pub const D_RESIDUAL_NO_EVENT: DResidual = DResidual::MAX;
pub const DELTA_T_RESIDUAL_NO_EVENT: DeltaTResidual = DeltaTResidual::MAX;

impl BlockIntraPredictionContextModel {
    fn new(delta_t_max: DeltaT) -> Self {
        let mut ret = Self {
            prev_coded_event: None,
            prediction_mode: TimeMode::AbsoluteT,
            d_model: BlockDResidualModel::new(),
            delta_t_model: BlockDeltaTResidualModel::new(delta_t_max),
            // d_encoder: Encoder::new(BlockDResidualModel::new(), &mut d_writer),
            // d_encoder: None,
            // d_writer,
        };

        ret
    }

    // Encode each event in the block in zigzag order. Context looks at the previous encoded event
    // to determine the residual.
    fn encode_block<'a, W>(&mut self, block: &mut Block, file_writer: &'a mut W)
    where
        W: BitWrite,
    {
        let mut d_writer = BitWriter::endian(Vec::new(), BigEndian);
        let mut d_encoder = Encoder::new(self.d_model.clone(), &mut d_writer); // Todo: shouldn't clone models unless at new AVU time point, ideally...
        let mut dt_writer = BitWriter::endian(Vec::new(), BigEndian);
        let mut dt_encoder = Encoder::new(self.delta_t_model.clone(), &mut dt_writer);

        let zigzag = ZigZag::new(block, &ZIGZAG_ORDER);
        for event in zigzag {
            self.encode_event(event, &mut d_encoder, &mut dt_encoder);
        }

        d_encoder.encode(None).unwrap();
        dt_encoder.encode(None).unwrap();
        file_writer.write_bytes(&d_writer.into_writer()).unwrap();
        file_writer.write_bytes(&dt_writer.into_writer()).unwrap();
    }

    // Encode the prediction residual for an event based on the previous coded event
    fn encode_event(
        &mut self,
        event: Option<&EventCoordless>,
        d_encoder: &mut Encoder<BlockDResidualModel, BitWriter<Vec<u8>, BigEndian>>,
        dt_encoder: &mut Encoder<BlockDeltaTResidualModel, BitWriter<Vec<u8>, BigEndian>>,
    ) {
        // If this is the first event in the block, encode it directly
        let (d_resid, dt_resid) = match self.prev_coded_event {
            None => match event {
                None => (D_RESIDUAL_NO_EVENT, DELTA_T_RESIDUAL_NO_EVENT), // TODO: test this. Need to expand alphabet
                Some(ev) => {
                    self.prev_coded_event = Some(*ev);
                    (ev.d as DResidual, ev.delta_t as DeltaTResidual)
                }
            },
            Some(prev_event) => match event {
                None => (D_RESIDUAL_NO_EVENT, DELTA_T_RESIDUAL_NO_EVENT),
                Some(ev) => {
                    let d_resid = ev.d as DResidual - prev_event.d as DResidual;

                    // Get the prediction error for delta_t based on the change in D
                    let dt_resid = ev.delta_t as DeltaTResidual
                        - match d_resid {
                            0 => prev_event.delta_t as DeltaTResidual,
                            1_i16..=i16::MAX => {
                                if d_resid as u32 <= prev_event.delta_t.leading_zeros() / 2 {
                                    min(
                                        (prev_event.delta_t << d_resid).into(),
                                        self.delta_t_model.delta_t_max,
                                    )
                                } else {
                                    prev_event.delta_t.into()
                                }
                            }
                            i16::MIN..=-1_i16 => {
                                if -d_resid as u32 <= 32 - prev_event.delta_t.leading_zeros() {
                                    max(
                                        (prev_event.delta_t >> -d_resid).into(),
                                        prev_event.delta_t.into(),
                                    )
                                } else {
                                    prev_event.delta_t.into()
                                }
                            }
                        };

                    self.prev_coded_event = Some(*ev);
                    (d_resid, dt_resid)
                }
            },
        };

        d_encoder.encode(Some(&d_resid)).unwrap();
        dt_encoder.encode(Some(&dt_resid)).unwrap();
    }
}

// impl Model for BlockIntraPredictionContextModel {
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
    use crate::codec::compressed::blocks::Cube;
    use crate::codec::compressed::compression::{
        BlockDResidualModel, BlockDeltaTResidualModel, BlockIntraPredictionContextModel, DResidual,
        DeltaTResidual,
    };
    use crate::codec::compressed::BLOCK_SIZE_BIG;
    use crate::{Coord, Event};
    use arithmetic_coding::{Decoder, Encoder};
    use bitstream_io::{BigEndian, BitReader, BitWrite, BitWriter};
    use rand::Rng;
    use std::fs::File;

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
        println!("{output:?}");
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
        println!("{output:?}");
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
        println!("{output:?}");
        assert_eq!(output, input);
    }

    struct Setup {
        cube: Cube,
        event: Event,
        events_for_block_r: Vec<Event>,
        events_for_block_g: Vec<Event>,
        events_for_block_b: Vec<Event>,
    }
    impl Setup {
        fn new() -> Self {
            let mut rng = rand::thread_rng();
            let mut events_for_block_r = Vec::new();
            for y in 0..BLOCK_SIZE_BIG {
                for x in 0..BLOCK_SIZE_BIG {
                    events_for_block_r.push(Event {
                        coord: Coord {
                            y: y as u16,
                            x: x as u16,
                            c: Some(0),
                        },

                        d: rng.gen_range(0..20),
                        delta_t: rng.gen_range(1..2550),
                    });
                }
            }

            let mut events_for_block_g = Vec::new();
            for y in 0..BLOCK_SIZE_BIG {
                for x in 0..BLOCK_SIZE_BIG {
                    events_for_block_g.push(Event {
                        coord: Coord {
                            y: y as u16,
                            x: x as u16,
                            c: Some(1),
                        },
                        ..Default::default()
                    });
                }
            }

            let mut events_for_block_b = Vec::new();
            for y in 0..BLOCK_SIZE_BIG {
                for x in 0..BLOCK_SIZE_BIG {
                    events_for_block_b.push(Event {
                        coord: Coord {
                            y: y as u16,
                            x: x as u16,
                            c: Some(2),
                        },
                        ..Default::default()
                    });
                }
            }

            Self {
                cube: Cube::new(0, 0, 0),
                event: Event {
                    coord: Coord {
                        x: 0,
                        y: 0,
                        c: Some(0),
                    },
                    d: 7,
                    delta_t: 100,
                },
                events_for_block_r,
                events_for_block_g,
                events_for_block_b,
            }
        }
    }

    #[test]
    fn test_encode_block() {
        let mut context_model = BlockIntraPredictionContextModel::new(2550);
        let setup = Setup::new();
        let mut cube = setup.cube;
        let events = setup.events_for_block_r;

        for event in events.iter() {
            assert!(cube.set_event(*event).is_ok());
        }

        let mut out_writer = BitWriter::endian(Vec::new(), BigEndian);

        context_model.encode_block(&mut cube.blocks_r[0], &mut out_writer);
    }
}
