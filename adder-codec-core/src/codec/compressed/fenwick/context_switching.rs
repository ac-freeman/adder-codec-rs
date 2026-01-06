// From https://github.com/danieleades/arithmetic-coding. Only temporary, for initial testing.
//! Fenwick tree based context-switching model

use arithmetic_coding::Model;

use super::Weights;
use crate::codec::compressed::fenwick::ValueError;

#[derive(Debug, Clone)]
pub struct FenwickModel {
    contexts: Vec<Weights>,
    current_context: usize,
    max_denominator: u64,
}

impl FenwickModel {
    /// Create a new model with default contexts having the given number of symbols
    #[must_use]
    pub fn with_symbols(symbols: usize, max_denominator: u64) -> Self {
        // let mut contexts = Vec::with_capacity(symbols + 1);
        let mut contexts = Vec::with_capacity(10);

        // for _ in 0..=symbols {
        contexts.push(Weights::new(symbols));
        // }

        Self {
            contexts,
            current_context: 0,
            max_denominator,
        }
    }

    /// Push a new context onto the stack, of the given size
    pub fn push_context(&mut self, symbols: usize) -> (usize, &mut Weights) {
        self.contexts.push(Weights::new(symbols));
        let index = self.contexts.len() - 1;
        (index, &mut self.contexts[index])
    }

    /// Push a new context onto the stack, of the given size
    pub fn push_context_with_weights(&mut self, weights: Weights) -> usize {
        self.contexts.push(weights);

        self.contexts.len() - 1
    }

    pub fn set_context(&mut self, context: usize) {
        self.current_context = context;
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

        /*
        Commented out so that context switching isn't automatically done. Thus, the contexts here
        are managed manually, and not based on the previous symbols of the same type.
        TODO: maintain a 2d array of contexts, where the first dimension is the type of symbol (e.g.
          D, delta_t), and the second dimension is the context. Then, when a symbol is updated,
          update the contexts of the same type. Possibly use the encoder chain function? Need to modify it...
         */
        // self.current_context = symbol.map(|x| x + 1).unwrap_or_default();
    }
}
