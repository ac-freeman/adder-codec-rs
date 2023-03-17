use arithmetic_coding::{Decoder, Encoder};
use bitstream_io::{BigEndian, BitReader, BitWrite, BitWriter};
use std::cmp::{max, min};

use std::io::{BufReader, BufWriter};

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

use crate::codec::compressed::blocks::D_ENCODE_NO_EVENT;
use crate::codec_old::compressed::blocks::{Block, ZigZag, ZIGZAG_ORDER};
use crate::codec_old::compressed::fenwick::{context_switching::FenwickModel, Weights};
use crate::{DeltaT, EventCoordless, D};

pub type DResidual = i16;
pub type DeltaTResidual = i64;
pub type DeltaTResidualSmall = i16;
pub const DELTA_T_RESIDUAL_NO_EVENT: DeltaTResidual = DeltaTResidual::MAX; // TODO: document and test

// static D_RESIDUAL_DEFAULT_WEIGHTS: Weights = d_residual_default_weights();

pub fn d_residual_default_weights() -> Weights {
    // todo: what about d_no_event... 256?
    // The maximum positive d residual is d = 0 --> d = 255      [255]
    // The maximum negative d residual is d = 255 --> d = 0      [-255]
    // No d values in range (D_MAX, D_NO_EVENT) --> (173, 253)

    // Span the range [-255, 256]
    let mut counts: [u64; 512] = [1; 512];

    // Give high probability to range [-20, 20]
    let mut idx = 0;
    loop {
        match idx {
            // [-10, 10]
            245..=265 => {
                counts[idx] = 20;
            }

            // [-20, 20]
            235..=275 | 490..=510 | 0..=20 => {
                counts[idx] = 10;
            }

            // give high probability to d_no_event
            511 => {
                counts[idx] = 20;
            }
            _ => {}
        }

        if idx == 511 {
            break;
        }

        idx += 1;
    }

    Weights::new_with_counts(counts.len(), &Vec::from(counts))
}

pub fn dt_residual_default_weights(delta_t_max: DeltaT, delta_t_ref: DeltaT) -> Weights {
    let min: usize = delta_t_max as usize;
    let mut counts: Vec<u64> = vec![1; (delta_t_max * 2) as usize + 2]; // +1 for 0, +1 for EOF

    // Give high probability to range [-delta_t_ref, delta_t_ref]
    let slice =
        &mut counts[(-(delta_t_ref as i64) + min as i64) as usize..(delta_t_ref as usize) + min];
    for count in slice {
        *count = 20;
    }

    let tmp = Weights::new_with_counts(counts.len(), &counts);
    assert_eq!(tmp.range(None), 0..1);
    tmp
}

pub struct Contexts {
    pub(crate) d_context: usize,
    pub(crate) dt_context: usize,
    pub(crate) eof_context: usize,
}

impl Contexts {
    pub fn new(d_context: usize, dt_context: usize, eof_context: usize) -> Contexts {
        // Initialize weights for d_context

        Contexts {
            d_context,
            dt_context,
            eof_context,
        }
    }
}

pub struct CompressionModelEncoder<W: std::io::Write + std::fmt::Debug> {
    contexts: Contexts,
    // d_context
    // dt_context
    //
    prev_coded_event: Option<EventCoordless>,
    delta_t_max: DeltaT,
    pub bitwriter: BitWriter<BufWriter<W>, BigEndian>,
    encoder: arithmetic_coding::Encoder<FenwickModel, BitWriter<BufWriter<W>, BigEndian>>,
    d_resid: DResidual,
    dt_resid: DeltaTResidual,
    // pub bitreader: Option<BitReader<BufReader<R>, BigEndian>>,
}
impl<W: std::io::Write + std::fmt::Debug> CompressionModelEncoder<W> {
    pub fn new(delta_t_max: DeltaT, delta_t_ref: DeltaT, writer: BufWriter<W>) -> Self {
        let bitwriter = BitWriter::endian(writer, BigEndian);

        // How many symbols we need to account for in the maximum
        let _num_symbols = delta_t_max as usize * 2;

        let mut source_model = FenwickModel::with_symbols(delta_t_max as usize * 2, 1 << 30);

        // D context. Only need to account for range [-255, 255]
        let d_context_idx = source_model.push_context_with_weights(d_residual_default_weights());

        // Delta_t context. Need to account for range [-delta_t_max, delta_t_max]
        let dt_context_idx = source_model
            .push_context_with_weights(dt_residual_default_weights(delta_t_max, delta_t_ref));

        let eof_context_idx =
            source_model.push_context_with_weights(Weights::new_with_counts(1, &vec![1]));

        let contexts = Contexts::new(d_context_idx, dt_context_idx, eof_context_idx);

        let encoder = Encoder::new(source_model);

        CompressionModelEncoder {
            contexts,
            prev_coded_event: None,
            delta_t_max,
            bitwriter,
            encoder,
            d_resid: 0,
            dt_resid: 0,
        }
    }

    pub fn encode_block(&mut self, block: &mut Block) {
        self.prev_coded_event = None;
        let zigzag = ZigZag::new(block, &ZIGZAG_ORDER);
        for event in zigzag {
            self.encode_event(event);
        }
    }

    pub fn flush_encoder(&mut self) {
        // Encode the EOF symbol // TODO: only do this at AVU boundaries
        self.encoder.model.set_context(self.contexts.d_context);
        self.encoder.encode(None, &mut self.bitwriter).unwrap();
        // Must flush the encoder to the bitwriter before flushing the bitwriter itself
        self.encoder.flush(&mut self.bitwriter).unwrap();
        self.bitwriter.byte_align().unwrap();
        self.bitwriter.flush().unwrap();
    }

    /// Encode the prediction residual for an event based on the previous coded event
    pub fn encode_event(&mut self, event: Option<&EventCoordless>) {
        // If this is the first event in the block, encode it directly
        match (self.prev_coded_event, event) {
            (_, None) => {
                self.d_resid = D_ENCODE_NO_EVENT;
                self.dt_resid = DELTA_T_RESIDUAL_NO_EVENT
            } // TODO: test this. Need to expand alphabet
            (None, Some(ev)) => {
                self.prev_coded_event = Some(*ev);
                self.d_resid = ev.d as DResidual;
                self.dt_resid = ev.delta_t as DeltaTResidual;
            }
            (Some(prev_event), Some(ev)) => {
                self.d_resid = ev.d as DResidual - prev_event.d as DResidual;

                // Get the prediction error for delta_t based on the change in D
                self.dt_resid = ev.delta_t as DeltaTResidual
                    - match self.d_resid {
                        1_i16..=20_16 => {
                            // If D has increased by a little bit,
                            if self.d_resid as u32 <= prev_event.delta_t.leading_zeros() / 2 {
                                min(
                                    (prev_event.delta_t << self.d_resid) as DeltaTResidual,
                                    self.delta_t_max as DeltaTResidual,
                                )
                            } else {
                                prev_event.delta_t as DeltaTResidual
                            }
                        }
                        -20_i16..=-1_i16 => {
                            if -self.d_resid as u32 <= 32 - prev_event.delta_t.leading_zeros() {
                                max(
                                    (prev_event.delta_t >> -self.d_resid) as DeltaTResidual,
                                    prev_event.delta_t as DeltaTResidual,
                                )
                            } else {
                                prev_event.delta_t as DeltaTResidual
                            }
                        }
                        // If D has not changed, or has changed a whole lot, use the previous delta_t
                        _ => prev_event.delta_t as DeltaTResidual,
                    };

                self.prev_coded_event = Some(*ev);
                // eprintln!("d_resid: {}, dt_resid: {}", d_resid, dt_resid);
                // (d_resid, dt_resid)
            }
        };

        self.encoder.model.set_context(self.contexts.d_context);
        self.encoder
            .encode(Some(&d_resid_offset(self.d_resid)), &mut self.bitwriter)
            .unwrap();

        self.encoder.model.set_context(self.contexts.dt_context);
        self.encoder
            .encode(
                Some(&dt_resid_offset(self.dt_resid, self.delta_t_max)),
                &mut self.bitwriter,
            )
            .unwrap();
    }
}

/// Takes a d_resid value and shifts it to be an index for the probability table
#[inline(always)]
pub fn d_resid_offset(d_resid: DResidual) -> usize {
    (d_resid + 255) as usize
}

/// Takes a decoded d_resid symbol and returns the actual d_resid value
#[inline(always)]
pub fn d_resid_offset_inverse(d_resid_symbol: usize) -> DResidual {
    (d_resid_symbol as i64 - 255) as DResidual
}

#[inline(always)]
pub fn dt_resid_offset(dt_resid: DeltaTResidual, delta_t_max: DeltaT) -> usize {
    (dt_resid + delta_t_max as i64) as usize
}

/// Takes a dt_resid value and shifts it to be an index for the probability table
#[inline(always)]
pub fn dt_resid_offset_i16(dt_resid: DeltaTResidualSmall, delta_t_max: DeltaT) -> usize {
    if delta_t_max < i16::MAX as DeltaT {
        (dt_resid as i64 + delta_t_max as i64) as usize
    } else {
        (dt_resid as i64 - i16::MIN as i64) as usize
    }
}

/// Takes a decoded dt_resid symbol and returns the actual dt_resid value
#[inline(always)]
pub fn dt_resid_offset_i16_inverse(
    dt_resid_symbol: usize,
    delta_t_max: DeltaT,
) -> DeltaTResidualSmall {
    if delta_t_max < i16::MAX as DeltaT {
        (dt_resid_symbol as i64 - delta_t_max as i64) as DeltaTResidualSmall
    } else {
        (dt_resid_symbol as i64 + i16::MIN as i64) as DeltaTResidualSmall
    }
}

pub struct CompressionModelDecoder<R: std::io::Read> {
    contexts: Contexts,
    prev_decoded_event: Option<EventCoordless>,
    delta_t_max: DeltaT,
    pub bitreader: BitReader<BufReader<R>, BigEndian>,
    decoder: Decoder<FenwickModel, BitReader<BufReader<R>, BigEndian>>,
}
impl<R: std::io::Read> CompressionModelDecoder<R> {
    pub fn new(delta_t_max: DeltaT, delta_t_ref: DeltaT, reader: BufReader<R>) -> Self {
        let bitreader = BitReader::endian(reader, BigEndian);

        // How many symbols we need to account for in the maximum
        let _num_symbols = delta_t_max as usize * 2;

        let mut source_model = FenwickModel::with_symbols(delta_t_max as usize * 2, 1 << 30);

        // D context. Only need to account for range [-255, 255]
        let d_context_idx = source_model.push_context_with_weights(d_residual_default_weights());

        // Delta_t context. Need to account for range [-delta_t_max, delta_t_max]
        let dt_context_idx = source_model
            .push_context_with_weights(dt_residual_default_weights(delta_t_max, delta_t_ref));

        let eof_context_idx =
            source_model.push_context_with_weights(Weights::new_with_counts(1, &vec![1]));

        let contexts = Contexts::new(d_context_idx, dt_context_idx, eof_context_idx);

        let decoder = Decoder::new(source_model);

        CompressionModelDecoder {
            contexts,
            prev_decoded_event: None,
            delta_t_max,
            bitreader,
            decoder,
        }
    }

    pub fn decode_block(&mut self, block: &mut Block) {
        // assert!(self.reader.is_some()); // TODO: return result
        self.prev_decoded_event = None;

        for idx in ZIGZAG_ORDER {
            self.decode_event(block, idx);
        }

        // self.dt_reader.byte_align();
    }

    pub fn check_eof(&mut self) {
        self.decoder.model.set_context(self.contexts.d_context);
        let d_resid_opt = self.decoder.decode(&mut self.bitreader).unwrap();
        assert!(d_resid_opt.is_none());
    }

    fn decode_event(&mut self, block_ref: &mut Block, idx: u16) {
        // Read the d residual
        self.decoder.model.set_context(self.contexts.d_context);
        let d_resid = self.decoder.decode(&mut self.bitreader).unwrap().unwrap();
        let d_resid = d_resid as i16 - 255;

        // Read the dt residual
        self.decoder.model.set_context(self.contexts.dt_context);
        let dt_resid = self.decoder.decode(&mut self.bitreader).unwrap().unwrap();
        let dt_resid = dt_resid as i64 - self.delta_t_max as i64;

        if d_resid == D_ENCODE_NO_EVENT {
            unsafe { *block_ref.events.get_unchecked_mut(idx as usize) = None };
            return;
        }

        let (d, dt) = match self.prev_decoded_event {
            None => {
                // let d_resid = self
                //     .d_decoder
                //     .decode(self.bitreader.as_mut().unwrap())
                //     .unwrap()
                //     .unwrap();
                // let dt_resid = self
                //     .dt_decoder
                //     .decode(self.bitreader.as_mut().unwrap())
                //     .unwrap()
                //     .unwrap();

                // eprintln!("idx: {}, d_resid: {}, dt_resid: {}", idx, d_resid, dt_resid);
                (d_resid, dt_resid)
            }
            Some(prev_event) => {
                // let d_resid = self
                //     .d_decoder
                //     .decode(self.bitreader.as_mut().unwrap())
                //     .unwrap()
                //     .unwrap();
                // let dt_resid = self
                //     .dt_decoder
                //     .decode(self.bitreader.as_mut().unwrap())
                //     .unwrap()
                //     .unwrap();

                let dt_pred = match d_resid {
                    1_i16..=20_i16 => {
                        if d_resid as u32 <= prev_event.delta_t.leading_zeros() / 2 {
                            min(
                                (prev_event.delta_t << d_resid).into(),
                                self.delta_t_max.into(),
                            )
                        } else {
                            prev_event.delta_t.into()
                        }
                    }
                    -20_i16..=-1_i16 => {
                        if -d_resid as u32 <= 32 - prev_event.delta_t.leading_zeros() {
                            max(
                                (prev_event.delta_t >> -d_resid).into(),
                                prev_event.delta_t.into(),
                            )
                        } else {
                            prev_event.delta_t.into()
                        }
                    }
                    _ => prev_event.delta_t as DeltaTResidual,
                };
                // eprintln!("idx: {}, d_resid: {}, dt_resid: {}", idx, d_resid, dt_resid);
                (d_resid + prev_event.d as i16, dt_pred + dt_resid)
            }
        };

        let event = match d {
            D_RESIDUAL_NO_EVENT => None,
            _ => {
                let event = EventCoordless {
                    d: d as D,
                    delta_t: dt as DeltaT,
                };
                self.prev_decoded_event = Some(event);
                Some(event)
            }
        };

        unsafe { *block_ref.events.get_unchecked_mut(idx as usize) = event };
    }
}

#[cfg(test)]
mod tests {
    use crate::codec_old::compressed::blocks::Cube;
    use crate::codec_old::compressed::compression::{
        CompressionModelDecoder, CompressionModelEncoder,
    };
    use crate::codec_old::compressed::{BLOCK_SIZE_BIG, BLOCK_SIZE_BIG_AREA};

    use rand::prelude::StdRng;
    use rand::{Rng, SeedableRng};

    use crate::{Coord, Event};
    use std::io::{BufReader, BufWriter, Write};

    struct Setup {
        cube: Cube,
        event: Event,
        events_for_block_r: Vec<Event>,
        events_for_block_g: Vec<Event>,
        events_for_block_b: Vec<Event>,
    }
    impl Setup {
        fn new(seed: Option<u64>) -> Self {
            let mut rng = match seed {
                None => StdRng::from_rng(rand::thread_rng()).unwrap(),
                Some(num) => StdRng::seed_from_u64(num),
            };
            //
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
    fn test_encode_decode_block() {
        let setup = Setup::new(Some(473829479));
        let mut cube = setup.cube;
        let events = setup.events_for_block_r;

        for event in events.iter() {
            assert!(cube.set_event(*event).is_ok());
        }

        let mut write_result = Vec::new();
        let out_writer = BufWriter::new(&mut write_result);

        let mut model = CompressionModelEncoder::new(2550, 255, out_writer);

        model.encode_block(&mut cube.blocks_r[0]);
        model.flush_encoder();

        let mut writer = model.bitwriter.into_writer();
        writer.flush().unwrap();
        // let writer: &[u8] = &*out_writer.into_writer();

        let written = writer.into_inner().unwrap();
        let len = written.len();
        assert!(len < BLOCK_SIZE_BIG_AREA * 5); // 5 bytes per raw event when just encoding D and Dt
        println!("{len}");

        println!("input bytes: {}", BLOCK_SIZE_BIG_AREA * 5);
        println!("output bytes: {len}");

        println!(
            "compression ratio: {}",
            (BLOCK_SIZE_BIG_AREA * 5) as f32 / len as f32
        );

        let buf_reader = BufReader::new(&**written);

        let mut context_model = CompressionModelDecoder::new(2550, 255, buf_reader);

        context_model.decode_block(&mut cube.blocks_r[0]);
        context_model.check_eof();

        for idx in 0..BLOCK_SIZE_BIG_AREA {
            let source_d = events[idx].d;
            let source_dt = events[idx].delta_t;

            let decoded_d = cube.blocks_r[0].events[idx].unwrap().d;
            let decoded_dt = cube.blocks_r[0].events[idx].unwrap().delta_t;

            assert_eq!(source_d, decoded_d);
            assert_eq!(source_dt, decoded_dt);
        }
    }

    #[test]
    fn test_encode_decode_many_blocks() {
        let setup = Setup::new(Some(473829479));
        let mut cube = setup.cube;
        let events = setup.events_for_block_r;

        for event in events.iter() {
            assert!(cube.set_event(*event).is_ok());
        }

        let mut write_result = Vec::new();
        let out_writer = BufWriter::new(&mut write_result);

        let mut model = CompressionModelEncoder::new(255, 255, out_writer);

        let num_blocks = 1000;
        for _ in 0..num_blocks {
            model.encode_block(&mut cube.blocks_r[0]);
        }

        model.flush_encoder();
        let mut writer = model.bitwriter.into_writer();
        writer.flush().unwrap();
        // let writer: &[u8] = &*out_writer.into_writer();

        let written = writer.into_inner().unwrap();
        let len = written.len();
        assert!(len < BLOCK_SIZE_BIG_AREA * 5 * num_blocks); // 5 bytes per raw event when just encoding D and Dt
        println!("{len}");

        println!("input bytes: {}", BLOCK_SIZE_BIG_AREA * 5 * num_blocks);
        println!("output bytes: {len}");

        println!(
            "compression ratio: {}",
            (BLOCK_SIZE_BIG_AREA * 5 * num_blocks) as f32 / len as f32
        );

        // let buf_reader = BufReader::new(&**written);
        //
        // let mut context_model = CompressionModelDecoder::new(2550, 255, buf_reader);
        //
        // for _block_num in 0..num_blocks {
        //     context_model.decode_block(&mut cube.blocks_r[0]);
        //
        //     for idx in 0..BLOCK_SIZE_BIG_AREA {
        //         let source_d = events[idx].d;
        //         let source_dt = events[idx].delta_t;
        //
        //         let decoded_d = cube.blocks_r[0].events[idx].unwrap().d;
        //         let decoded_dt = cube.blocks_r[0].events[idx].unwrap().delta_t;
        //
        //         assert_eq!(source_d, decoded_d);
        //         assert_eq!(source_dt, decoded_dt);
        //     }
        // }
        // context_model.check_eof();
    }
}
