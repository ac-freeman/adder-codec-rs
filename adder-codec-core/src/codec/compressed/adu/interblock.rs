use crate::codec::compressed::blocks::{DResidual, BLOCK_SIZE_AREA};

pub struct AduInterBlock {
    /// How many bits the dt_residuals are shifted by.
    pub(crate) shift_loss_param: u8,

    /// Prediction residuals of D between each event and the event in the previous block.
    pub(crate) d_residuals: [DResidual; BLOCK_SIZE_AREA],

    /// Prediction residuals of delta_t between each event and the event in the previous block.
    pub(crate) t_residuals: [i16; BLOCK_SIZE_AREA],
}
