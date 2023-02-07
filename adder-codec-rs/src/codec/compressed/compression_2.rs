use arithmetic_coding::{Decoder, Encoder, Model};
use bitstream_io::{BigEndian, BitRead, BitReader, BitWrite, BitWriter};
use std::cmp::{max, min};
use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom};
use std::mem::swap;
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
use crate::codec::compressed::fenwick::{context_switching::FenwickModel, ValueError, Weights};
use crate::codec::compressed::{BLOCK_SIZE_BIG, BLOCK_SIZE_BIG_AREA};
use crate::framer::driver::EventCoordless;
use crate::{DeltaT, TimeMode, D};

// static D_RESIDUAL_DEFAULT_WEIGHTS: Weights = d_residual_default_weights();

fn d_residual_default_weights() -> Weights {
    let min: usize = 255;

    // The maximum positive d residual is d = 0 --> d = 255      [255]
    // The maximum negative d residual is d = 255 --> d = 0      [-255]
    // No d values in range (D_MAX, D_NO_EVENT) --> (173, 253)

    // Span the range [-255, 255]
    let mut counts: [u64; 511] = [1; 511];

    // Give high probability to range [-20, 20]
    let mut idx = 0;
    loop {
        match idx {
            // [-10, 10]
            245..=265 => {
                counts[idx] = 20;
            }

            // [-20, 20]
            235..=275 => {
                counts[idx] = 10;
            }

            // [235, 255]
            490..=510 => {
                counts[idx] = 10;
            }

            // [-255, -235]
            0..=20 => {
                counts[idx] = 10;
            }
            _ => {}
        }

        if idx == 510 {
            break;
        }

        idx += 1;
    }

    Weights::new_with_counts(counts.len(), Vec::from(counts))
}

fn dt_residual_default_weights(delta_t_max: DeltaT, delta_t_ref: DeltaT) -> Weights {
    let min: usize = delta_t_max as usize;
    let mut counts: Vec<u64> = vec![1; (delta_t_max * 2) as usize + 1];

    // Give high probability to range [-delta_t_ref, delta_t_ref]
    let slice =
        &mut counts[(-(delta_t_ref as i64) + min as i64) as usize..(delta_t_ref as usize) + min];
    for count in slice {
        *count = 20;
    }

    Weights::new_with_counts(counts.len(), counts)
}

struct Contexts {
    d_context: usize,
    dt_context: usize,
}

impl Contexts {
    fn new(d_context: usize, dt_context: usize) -> Contexts {
        // Initialize weights for d_context

        Contexts {
            d_context,
            dt_context,
        }
    }
}

pub struct CompressionModelEncoder<W: std::io::Write + std::fmt::Debug> {
    contexts: Contexts,
    // d_context
    // dt_context
    //
    pub bitwriter: BitWriter<BufWriter<W>, BigEndian>,
    encoder: Encoder<FenwickModel, BitWriter<BufWriter<Vec<u8>>, BigEndian>>,
    // pub bitreader: Option<BitReader<BufReader<R>, BigEndian>>,
}
impl<W: std::io::Write + std::fmt::Debug> CompressionModelEncoder<W> {
    pub fn new_encoder(delta_t_max: DeltaT, delta_t_ref: DeltaT, writer: BufWriter<W>) -> Self {
        let bitwriter = BitWriter::endian(writer, BigEndian);

        // How many symbols we need to account for in the maximum
        let num_symbols = delta_t_max as usize * 2;

        let mut source_model = FenwickModel::with_symbols(delta_t_max as usize * 2, 1 << 20);

        // D context. Only need to account for range [-255, 255]
        let (d_context_idx) = source_model.push_context_with_weights(d_residual_default_weights());

        // Delta_t context. Need to account for range [-delta_t_max, delta_t_max]
        let (dt_context_idx) = source_model.push_context_with_weights(
            dt_residual_default_weights(delta_t_max, delta_t_ref).clone(),
        );

        let contexts = Contexts::new(d_context_idx, dt_context_idx);

        let mut encoder = Encoder::new(source_model);

        CompressionModelEncoder {
            contexts,
            bitwriter,
            encoder,
        }
    }
}
