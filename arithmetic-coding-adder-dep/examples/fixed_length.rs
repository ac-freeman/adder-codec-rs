#![feature(exclusive_range_pattern)]
#![feature(never_type)]

use std::ops::Range;

use arithmetic_coding_adder_dep::fixed_length;

mod common;

#[derive(Debug)]
pub enum Symbol {
    A,
    B,
    C,
}

#[derive(Clone)]
pub struct MyModel;

impl fixed_length::Model for MyModel {
    type Symbol = Symbol;
    type ValueError = !;

    fn probability(&self, symbol: &Self::Symbol) -> Result<Range<u32>, Self::ValueError> {
        match symbol {
            Symbol::A => Ok(0..1),
            Symbol::B => Ok(1..2),
            Symbol::C => Ok(2..3),
        }
    }

    fn symbol(&self, value: u32) -> Self::Symbol {
        match value {
            0..1 => Symbol::A,
            1..2 => Symbol::B,
            2..3 => Symbol::C,
            _ => unreachable!(),
        }
    }

    fn max_denominator(&self) -> u32 {
        3
    }

    fn length(&self) -> usize {
        3
    }
}

fn main() {
    let input = vec![Symbol::A, Symbol::B, Symbol::C];
    let model = fixed_length::Wrapper::new(MyModel);

    common::round_trip(model, input);
}
