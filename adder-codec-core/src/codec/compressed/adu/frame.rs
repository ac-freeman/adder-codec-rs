//! An independtly decodable unit of video data.
//!
//! I try to lay out the struct here to be a pretty direct translation of the
//! compressed representation. That is, all the data in the struct is what you get when you
//! decompress an ADU.

use crate::codec::compressed::adu::cube::AduCube;
use crate::codec::compressed::blocks::{DResidual, BLOCK_SIZE_AREA};
use crate::{AbsoluteT, D};

pub struct AduChannel {
    /// The number of cubes in the ADU.
    num_cubes: u16,

    /// The cubes in the ADU.
    cubes: Vec<AduCube>,
}

impl AduChannel {
    fn compress() -> Vec<u8> {
        todo!()
    }

    fn decompress() -> Self {
        todo!()
    }
}

/// A whole spatial frame of data
pub struct Adu {
    /// The timestamp of the first event in the ADU.
    pub(crate) head_event_t: AbsoluteT,

    cubes_r: AduChannel,
    cubes_g: AduChannel,
    cubes_b: AduChannel,
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

    fn compress() -> Vec<u8> {
        todo!()
    }

    fn decompress() -> Self {
        todo!()
    }
}
