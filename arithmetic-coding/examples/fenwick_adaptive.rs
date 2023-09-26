use std::{fs::File, io::Read, ops::Range};

use arithmetic_coding::Model;

mod common;

use fenwick_model::{simple::FenwickModel, ValueError};

const ALPHABET: &str =
    "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789 .,\n-':()[]#*;\"!?*&é/àâè%@$";

#[derive(Debug, Clone)]
pub struct StringModel {
    alphabet: Vec<char>,
    fenwick_model: FenwickModel,
}

impl StringModel {
    #[must_use]
    pub fn new(alphabet: Vec<char>) -> Self {
        let fenwick_model = FenwickModel::builder(alphabet.len(), 1 << 20)
            .panic_on_saturation()
            .build();
        Self {
            alphabet,
            fenwick_model,
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("invalid character: {0}")]
pub struct Error(char);

impl Model for StringModel {
    type B = u64;
    type Symbol = char;
    type ValueError = ValueError;

    fn probability(
        &self,
        symbol: Option<&Self::Symbol>,
    ) -> Result<Range<Self::B>, Self::ValueError> {
        let fenwick_symbol = symbol.map(|c| self.alphabet.iter().position(|x| x == c).unwrap());
        self.fenwick_model.probability(fenwick_symbol.as_ref())
    }

    fn symbol(&self, value: Self::B) -> Option<Self::Symbol> {
        let index = self.fenwick_model.symbol(value)?;
        self.alphabet.get(index).copied()
    }

    fn max_denominator(&self) -> Self::B {
        self.fenwick_model.max_denominator()
    }

    fn denominator(&self) -> Self::B {
        self.fenwick_model.denominator()
    }

    fn update(&mut self, symbol: Option<&Self::Symbol>) {
        let fenwick_symbol = symbol.map(|c| self.alphabet.iter().position(|x| x == c).unwrap());
        self.fenwick_model.update(fenwick_symbol.as_ref());
    }
}

fn main() {
    let model = StringModel::new(ALPHABET.chars().collect());

    let mut input = String::new();
    File::open("./resources/sherlock.txt")
        .unwrap()
        .read_to_string(&mut input)
        .unwrap();

    common::round_trip_string(model, &input);
}
