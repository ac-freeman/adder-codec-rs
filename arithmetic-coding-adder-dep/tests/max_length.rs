#![feature(exclusive_range_pattern)]
#![feature(never_type)]

use std::ops::Range;

use arithmetic_coding_adder_dep::max_length;
use test_case::test_case;

mod common;

#[derive(Debug, PartialEq, Clone, Eq)]
pub enum Symbol {
    A,
    B,
    C,
}

#[derive(Clone)]
pub struct MyModel;

impl max_length::Model for MyModel {
    type Symbol = Symbol;
    type ValueError = !;

    fn probability(&self, symbol: Option<&Self::Symbol>) -> Result<Range<u32>, Self::ValueError> {
        match symbol {
            Some(Symbol::A) => Ok(0..1),
            Some(Symbol::B) => Ok(1..2),
            Some(Symbol::C) => Ok(2..3),
            None => Ok(3..4),
        }
    }

    fn symbol(&self, value: u32) -> Option<Self::Symbol> {
        match value {
            0..1 => Some(Symbol::A),
            1..2 => Some(Symbol::B),
            2..3 => Some(Symbol::C),
            3..4 => None,
            _ => unreachable!(),
        }
    }

    fn max_denominator(&self) -> u32 {
        4
    }

    fn max_length(&self) -> usize {
        3
    }
}

#[test_case(&[Symbol::A, Symbol::B] ; "shorter")]
#[test_case(&[Symbol::A, Symbol::B, Symbol::C] ; "exact")]
#[test_case(&[Symbol::A, Symbol::B, Symbol::C, Symbol::C] => panics ; "longer")]
fn round_trip(input: &[Symbol]) {
    common::round_trip(max_length::Wrapper::new(MyModel), input);
}
