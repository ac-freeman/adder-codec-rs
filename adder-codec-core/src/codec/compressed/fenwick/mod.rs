// From https://github.com/danieleades/arithmetic-coding. Only temporary, for initial testing.
//! [`Models`](crate::Model) implemented using Fenwick trees

use std::ops::Range;

pub mod context_switching;
pub mod simple;

/// A wrapper around a vector of fenwick counts, with one additional weight for
/// EOF.
#[derive(Debug, Clone)]
pub struct Weights {
    fenwick_counts: Vec<u64>,
    total: u64,
}

impl Weights {
    pub fn new(n: usize) -> Self {
        // we add one extra value here to account for the EOF
        let mut fenwick_counts = vec![0; n + 1];

        for i in 0..fenwick_counts.len() {
            fenwick::array::update(&mut fenwick_counts, i, 1);
        }

        let total = fenwick_counts.len() as u64;
        Self {
            fenwick_counts,
            total,
        }
    }

    /// Initialize the weights with the given counts
    pub fn new_with_counts(n: usize, counts: &Vec<u64>) -> Self {
        // we add one extra value here to account for the EOF (stored at the FIRST index)
        let fenwick_counts = vec![0; n + 1];

        let mut weights = Self {
            fenwick_counts,
            total: 0,
        };

        for (i, &count) in counts.iter().enumerate() {
            weights.update(Some(i), count);
        }
        weights.update(None, 1);
        weights
    }

    fn update(&mut self, i: Option<usize>, delta: u64) {
        let index = i.map(|i| i + 1).unwrap_or_default();
        fenwick::array::update(&mut self.fenwick_counts, index, delta);
        self.total += delta;
    }

    fn prefix_sum(&self, i: Option<usize>) -> u64 {
        let index = i.map(|i| i + 1).unwrap_or_default();
        fenwick::array::prefix_sum(&self.fenwick_counts, index)
    }

    /// Returns the probability range for the given symbol
    pub(crate) fn range(&self, i: Option<usize>) -> Range<u64> {
        // Increment the symbol index by one to account for the EOF?
        let index = i.map(|i| i + 1).unwrap_or_default();

        let upper = fenwick::array::prefix_sum(&self.fenwick_counts, index);

        let lower = if index == 0 {
            0
        } else {
            fenwick::array::prefix_sum(&self.fenwick_counts, index - 1)
        };
        lower..upper
    }

    pub fn len(&self) -> usize {
        self.fenwick_counts.len() - 1
    }

    /// Used for decoding. Find the symbol index for the given `prefix_sum`
    fn symbol(&self, prefix_sum: u64) -> Option<usize> {
        if prefix_sum < self.prefix_sum(None) {
            return None;
        }

        // invariant: low <= our answer < high
        // we seek the lowest number i such that prefix_sum(i) > prefix_sum
        let mut low = 0;
        let mut high = self.len();
        debug_assert!(low < high);
        debug_assert!(prefix_sum < self.prefix_sum(Some(high - 1)));
        while low + 1 < high {
            let i = (low + high - 1) / 2;
            if self.prefix_sum(Some(i)) > prefix_sum {
                // i could be our answer, so set high just above it.
                high = i + 1;
            } else {
                // i could not be our answer, so set low just above it.
                low = i + 1;
            }
        }
        Some(low)
    }

    const fn total(&self) -> u64 {
        self.total
    }
}

#[derive(Debug, thiserror::Error)]
#[error("invalid symbol received: {0}")]
pub struct ValueError(pub usize);

#[cfg(test)]
mod tests {
    use super::Weights;

    #[test]
    fn total() {
        let weights = Weights::new(3);
        assert_eq!(weights.total(), 4);
    }

    #[test]
    fn range() {
        let weights = Weights::new(3);
        assert_eq!(weights.range(None), 0..1);
        assert_eq!(weights.range(Some(0)), 1..2);
        assert_eq!(weights.range(Some(1)), 2..3);
        assert_eq!(weights.range(Some(2)), 3..4);
    }

    #[test]
    #[should_panic]
    fn range_out_of_bounds() {
        let weights = Weights::new(3);
        weights.range(Some(3));
    }

    #[test]
    fn symbol() {
        let weights = Weights::new(3);
        assert_eq!(weights.symbol(0), None);
        assert_eq!(weights.symbol(1), Some(0));
        assert_eq!(weights.symbol(2), Some(1));
        assert_eq!(weights.symbol(3), Some(2));
    }

    #[test]
    #[should_panic]
    fn symbol_out_of_bounds() {
        let weights = Weights::new(3);
        weights.symbol(4);
    }
}
