use std::ops::Range;

use arithmetic_coding_adder_dep::Model;

mod common;

#[derive(Clone)]
pub struct MyModel;

#[derive(Debug, thiserror::Error)]
#[error("invalid symbol: {0}")]
pub struct Error(u8);

impl Model for MyModel {
    type Symbol = u8;
    type ValueError = Error;

    fn probability(&self, symbol: Option<&Self::Symbol>) -> Result<Range<u32>, Error> {
        match symbol {
            None => Ok(0..1),
            Some(&1) => Ok(1..2),
            Some(&2) => Ok(2..3),
            Some(&3) => Ok(3..4),
            Some(x) => Err(Error(*x)),
        }
    }

    fn symbol(&self, value: u32) -> Option<Self::Symbol> {
        match value {
            0..1 => None,
            1..2 => Some(1),
            2..3 => Some(2),
            3..4 => Some(3),
            _ => unreachable!(),
        }
    }

    fn max_denominator(&self) -> u32 {
        4
    }
}

fn main() {
    common::round_trip(MyModel, vec![2, 1, 1, 2, 2, 3, 1]);
}
