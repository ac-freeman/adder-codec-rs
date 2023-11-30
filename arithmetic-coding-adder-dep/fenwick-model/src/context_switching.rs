//! Fenwick tree based context-switching model

use arithmetic_coding_core::Model;

use super::Weights;
use crate::ValueError;

#[derive(Debug, Clone)]
pub struct FenwickModel {
    contexts: Vec<Weights>,
    current_context: usize,
    max_denominator: u64,
}

impl FenwickModel {
    #[must_use]
    pub fn with_symbols(symbols: usize, max_denominator: u64) -> Self {
        let mut contexts = Vec::with_capacity(symbols + 1);

        for _ in 0..=symbols {
            contexts.push(Weights::new(symbols));
        }

        Self {
            contexts,
            current_context: 1,
            max_denominator,
        }
    }

    fn context(&self) -> &Weights {
        &self.contexts[self.current_context]
    }

    fn context_mut(&mut self) -> &mut Weights {
        &mut self.contexts[self.current_context]
    }
}

impl Model for FenwickModel {
    type B = u64;
    type Symbol = usize;
    type ValueError = ValueError;

    fn probability(&self, symbol: Option<&usize>) -> Result<std::ops::Range<u64>, ValueError> {
        Ok(self.context().range(symbol.copied()))
    }

    fn denominator(&self) -> u64 {
        self.context().total
    }

    fn max_denominator(&self) -> u64 {
        self.max_denominator
    }

    fn symbol(&self, value: u64) -> Option<usize> {
        self.context().symbol(value)
    }

    fn update(&mut self, symbol: Option<&usize>) {
        debug_assert!(
            self.denominator() < self.max_denominator,
            "hit max denominator!"
        );
        if self.denominator() < self.max_denominator {
            self.context_mut().update(symbol.copied(), 1);
        }
        self.current_context = symbol.map(|x| x + 1).unwrap_or_default();
    }
}
