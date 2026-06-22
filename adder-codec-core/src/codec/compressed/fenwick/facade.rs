use crate::codec::compressed::fenwick::context_switching::FenwickModel;
use crate::codec::compressed::source_model::cabac_contexts::{
    Contexts, BITSHIFT_ENCODE_FULL, D_RESIDUAL_OFFSET,
};
use crate::codec::compressed::{DResidual, TResidual, DRESIDUAL_NO_EVENT, DRESIDUAL_SKIP_CUBE};
use crate::{AbsoluteT, DeltaT};
use arithmetic_coding::Model;
use std::mem::size_of;

const D_RESIDUAL_BYTES: usize = size_of::<DResidual>();

#[derive(Debug, Clone)]
enum Phase {
    Header {
        bytes_remaining: usize,
    },
    IntraD,
    IntraBitshift,
    IntraT {
        bytes_remaining: usize,
    },
    InterD {
        bytes_remaining: usize,
        d_buf: [u8; 2],
    },
    InterBitshift,
    InterT {
        bytes_remaining: usize,
    },
    Eof,
}

impl Phase {
    fn context(&self, contexts: &Contexts) -> usize {
        match self {
            Phase::Header { .. } | Phase::IntraT { .. } | Phase::InterT { .. } => {
                contexts.t_context
            }
            Phase::IntraD | Phase::InterD { .. } => contexts.d_context,
            Phase::IntraBitshift | Phase::InterBitshift => contexts.bitshift_context,
            Phase::Eof => contexts.eof_context,
        }
    }
}

#[derive(Debug)]
pub struct FacadeModel {
    inner: FenwickModel,
    contexts: Contexts,
    phase: Phase,
}

impl FacadeModel {
    #[must_use]
    pub fn new(dt_ref: DeltaT, max_denominator: u64) -> Self {
        let mut inner = FenwickModel::with_symbols(u16::MAX as usize, max_denominator);
        let contexts = Contexts::new(&mut inner, dt_ref);
        let phase = Phase::Header {
            bytes_remaining: size_of::<AbsoluteT>(),
        };
        let mut model = Self {
            inner,
            contexts,
            phase,
        };
        model.inner.set_context(model.contexts.t_context);
        model
    }

    pub fn contexts(&self) -> &Contexts {
        &self.contexts
    }

    pub fn begin_intra(&mut self) {
        self.phase = Phase::IntraD;
        self.inner.set_context(self.contexts.d_context);
    }

    pub fn begin_inter(&mut self) {
        self.phase = Phase::InterD {
            bytes_remaining: D_RESIDUAL_BYTES,
            d_buf: [0; 2],
        };
        self.inner.set_context(self.contexts.d_context);
    }

    pub fn begin_eof(&mut self) {
        self.phase = Phase::Eof;
        self.inner.set_context(self.contexts.eof_context);
    }

    fn advance_phase(&mut self, symbol: Option<&usize>) {
        let symbol = symbol.copied();
        let phase = std::mem::replace(&mut self.phase, Phase::Eof);
        let next_phase = match phase {
            Phase::Header {
                mut bytes_remaining,
            } => {
                bytes_remaining = bytes_remaining.saturating_sub(1);
                Phase::Header { bytes_remaining }
            }
            Phase::IntraD => match symbol {
                Some(value) => {
                    let no_event = (DRESIDUAL_NO_EVENT + D_RESIDUAL_OFFSET) as usize;
                    let skip = (DRESIDUAL_SKIP_CUBE + D_RESIDUAL_OFFSET) as usize;
                    if value == no_event || value == skip {
                        Phase::IntraD
                    } else {
                        Phase::IntraBitshift
                    }
                }
                None => Phase::IntraD,
            },
            Phase::IntraBitshift => match symbol {
                Some(value) => Phase::IntraT {
                    bytes_remaining: t_residual_bytes(value),
                },
                None => Phase::IntraBitshift,
            },
            Phase::IntraT {
                mut bytes_remaining,
            } => {
                bytes_remaining = bytes_remaining.saturating_sub(1);
                if bytes_remaining == 0 {
                    Phase::IntraD
                } else {
                    Phase::IntraT { bytes_remaining }
                }
            }
            Phase::InterD {
                mut bytes_remaining,
                mut d_buf,
            } => match symbol {
                Some(value) => {
                    let byte = value as u8;
                    if bytes_remaining == D_RESIDUAL_BYTES {
                        d_buf[0] = byte;
                        bytes_remaining -= 1;
                        Phase::InterD {
                            bytes_remaining,
                            d_buf,
                        }
                    } else {
                        d_buf[1] = byte;
                        let d_residual = DResidual::from_be_bytes(d_buf);
                        if d_residual == DRESIDUAL_NO_EVENT {
                            Phase::InterD {
                                bytes_remaining: D_RESIDUAL_BYTES,
                                d_buf: [0; 2],
                            }
                        } else {
                            Phase::InterBitshift
                        }
                    }
                }
                None => Phase::InterD {
                    bytes_remaining,
                    d_buf,
                },
            },
            Phase::InterBitshift => match symbol {
                Some(value) => Phase::InterT {
                    bytes_remaining: t_residual_bytes(value),
                },
                None => Phase::InterBitshift,
            },
            Phase::InterT {
                mut bytes_remaining,
            } => {
                bytes_remaining = bytes_remaining.saturating_sub(1);
                if bytes_remaining == 0 {
                    Phase::InterD {
                        bytes_remaining: D_RESIDUAL_BYTES,
                        d_buf: [0; 2],
                    }
                } else {
                    Phase::InterT { bytes_remaining }
                }
            }
            Phase::Eof => Phase::Eof,
        };
        self.phase = next_phase;
        let context = self.phase.context(&self.contexts);
        self.inner.set_context(context);
    }
}

impl Model for FacadeModel {
    type B = u64;
    type Symbol = usize;
    type ValueError = crate::codec::compressed::fenwick::ValueError;

    fn probability(
        &self,
        symbol: Option<&Self::Symbol>,
    ) -> Result<std::ops::Range<Self::B>, Self::ValueError> {
        self.inner.probability(symbol)
    }

    fn denominator(&self) -> Self::B {
        self.inner.denominator()
    }

    fn max_denominator(&self) -> Self::B {
        self.inner.max_denominator()
    }

    fn symbol(&self, value: Self::B) -> Option<Self::Symbol> {
        self.inner.symbol(value)
    }

    fn update(&mut self, symbol: Option<&Self::Symbol>) {
        self.inner.update(symbol);
        self.advance_phase(symbol);
    }
}

fn t_residual_bytes(bitshift_value: usize) -> usize {
    if bitshift_value as u8 == BITSHIFT_ENCODE_FULL {
        size_of::<i64>()
    } else {
        size_of::<TResidual>()
    }
}
