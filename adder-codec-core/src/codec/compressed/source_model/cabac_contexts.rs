use crate::codec::compressed::fenwick::context_switching::FenwickModel;
use crate::codec::compressed::fenwick::Weights;
use crate::codec::CodecMetadata;
use crate::DeltaT;
use arithmetic_coding::Encoder;
use bitstream_io::{BigEndian, BitWrite, BitWriter};

pub struct Contexts {
    /// Decimation factor residuals context
    pub(crate) d_context: usize,

    /// Dt_ref interval count residuals context (how many dt_ref intervals away is our predicted interval from the actual)
    pub(crate) dtref_context: usize,

    /// Timestamp residuals context
    pub(crate) t_context: usize,

    /// EOF context
    pub(crate) eof_context: usize,
}

impl Contexts {
    pub fn new(source_model: &mut FenwickModel, ref_interval: DeltaT) -> Contexts {
        let d_context = source_model.push_context_with_weights(d_residual_default_weights());
        let dtref_context = source_model.push_context_with_weights(d_residual_default_weights());
        let t_context =
            source_model.push_context_with_weights(t_residual_default_weights(ref_interval));
        let eof_context =
            source_model.push_context_with_weights(Weights::new_with_counts(1, &vec![1]));

        Contexts {
            d_context,
            dtref_context,
            t_context,
            eof_context,
        }
    }
}

pub fn t_residual_default_weights(dt_ref: DeltaT) -> Weights {
    // t residuals can fit within i16

    // After we've indexed into the correct interval, our timestamp residual can span [-dt_ref, dt_ref]

    // We have dt_max/dt_ref count of intervals per adu
    let mut counts: Vec<u64> = vec![1; u16::MAX as usize];
    // let mut counts: Vec<u64> = vec![1; (dt_ref * 2 + 1) as usize];

    // Give higher probability to smaller residuals
    for i in counts.len() / 3..counts.len() * 2 / 3 {
        counts[i] = 5;
    }
    let len = counts.len();
    counts[len / 2] = 10;

    Weights::new_with_counts(counts.len(), &counts)
}

// pub fn dtref_residual_default_weights(dt_ref: DeltaT, dt_max: DeltaT) -> Weights {
//     // dtref residuals can fit within i16
//
//     // We have dt_max/dt_ref count of intervals per adu
//     let mut counts: Vec<u64> = vec![1; (dt_max / dt_ref) as usize * 2 + 1];
//
//     // Give higher probability to smaller residuals
//     for i in counts.len() / 3..counts.len() * 2 / 3 {
//         counts[i] = 5;
//     }
//
//     let len = counts.len();
//     counts[len / 2] = 10;
//
//     Weights::new_with_counts(counts.len(), &counts)
// }

pub fn d_residual_default_weights() -> Weights {
    // d residuals can fit within i16

    // DResidual_NO_EVENT =  256
    // DResidual_SKIP_CUBE =  257
    // The maximum positive d residual is d = 0 --> d = 255      [255]
    // The maximum negative d residual is d = 255 --> d = 0      [-255]
    // No d values in range (D_MAX, D_NO_EVENT) --> (173, 253)

    // Span the range [-255, 257]
    let mut counts: [u64; 513] = [1; 513];

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

            // give high probability to skip cube
            512 => {
                counts[idx] = 10;
            }
            _ => {}
        }

        if idx == counts.len() - 1 {
            break;
        }

        idx += 1;
    }

    Weights::new_with_counts(counts.len(), &Vec::from(counts))
}

pub fn eof_context(
    contexts: &Contexts,
    encoder: &mut Encoder<FenwickModel, BitWriter<Vec<u8>, BigEndian>>,
    stream: &mut BitWriter<Vec<u8>, BigEndian>,
) {
    // THIS IS CRUCIAL FOR TESTING
    let eof_context = contexts.eof_context;
    encoder.model.set_context(eof_context);
    encoder.encode(None, stream).unwrap();
    encoder.flush(stream).unwrap();
    stream.byte_align().unwrap();
    stream.flush().unwrap();
}
