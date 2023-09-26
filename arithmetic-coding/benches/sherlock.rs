use std::{fs::File, io::Read, ops::Range};

use arithmetic_coding::Model;
use fenwick_model::{simple::FenwickModel, ValueError};

mod common;

#[derive(Debug, Clone)]
pub struct StringModel {
    fenwick_model: FenwickModel,
}

impl StringModel {
    #[must_use]
    pub fn new(symbols: usize) -> Self {
        let fenwick_model = FenwickModel::builder(symbols, 1 << 20)
            .panic_on_saturation()
            .build();
        Self { fenwick_model }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("invalid character: {0}")]
pub struct Error(char);

impl Model for StringModel {
    type B = u64;
    type Symbol = u8;
    type ValueError = ValueError;

    fn probability(
        &self,
        symbol: Option<&Self::Symbol>,
    ) -> Result<Range<Self::B>, Self::ValueError> {
        let fenwick_symbol = symbol.map(|c| *c as usize);
        self.fenwick_model.probability(fenwick_symbol.as_ref())
    }

    fn symbol(&self, value: Self::B) -> Option<Self::Symbol> {
        self.fenwick_model.symbol(value).map(|x| x as u8)
    }

    fn max_denominator(&self) -> Self::B {
        self.fenwick_model.max_denominator()
    }

    fn denominator(&self) -> Self::B {
        self.fenwick_model.denominator()
    }
}

fn round_trip(input: &[u8]) {
    let model = StringModel::new(256);

    common::round_trip(model, input);
}

use criterion::{black_box, criterion_group, criterion_main, Criterion};

#[allow(clippy::missing_panics_doc)]
pub fn criterion_benchmark(c: &mut Criterion) {
    let mut input_string = String::new();
    File::open("./resources/sherlock.txt")
        .unwrap()
        .read_to_string(&mut input_string)
        .unwrap();

    let truncated: String = input_string.chars().take(3428).collect();
    let input = truncated.as_bytes();

    c.bench_function("round trip", |b| b.iter(|| round_trip(black_box(input))));
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
