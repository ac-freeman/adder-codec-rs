#![feature(exclusive_range_pattern)]
#![feature(never_type)]

use std::ops::Range;

use arithmetic_coding::fixed_length;

mod common;

#[derive(Debug, PartialEq, Clone, Eq)]
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

#[test]
fn round_trip() {
    let input = &[Symbol::A, Symbol::B, Symbol::C];

    common::round_trip(fixed_length::Wrapper::new(MyModel), input);
}

#[test]
#[should_panic]
fn round_trip_fail() {
    // this is too many symbols for this model
    let input = &[
        Symbol::A,
        Symbol::B,
        Symbol::C,
        Symbol::A,
        Symbol::B,
        Symbol::C,
    ];

    common::round_trip(fixed_length::Wrapper::new(MyModel), input);
}
