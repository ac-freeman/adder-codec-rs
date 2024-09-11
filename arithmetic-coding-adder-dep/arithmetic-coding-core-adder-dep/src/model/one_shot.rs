//! Helper trait for creating Models which only accept a single symbol

use std::ops::Range;

pub use crate::fixed_length::Wrapper;
use crate::{fixed_length, BitStore};

/// A [`Model`] is used to calculate the probability of a given symbol occuring
/// in a sequence. The [`Model`] is used both for encoding and decoding. A
/// 'fixed-length' model always expects an exact number of symbols, and so does
/// not need to encode an EOF symbol.
///
/// A fixed length model can be converted into a regular model using the
/// convenience [`Wrapper`] type.
///
/// The more accurately a [`Model`] is able to predict the next symbol, the
/// greater the compression ratio will be.
///
/// # Example
///
/// ```
/// #![feature(exclusive_range_pattern)]
/// #![feature(never_type)]
/// # use std::ops::Range;
/// #
/// # use arithmetic_coding_core::one_shot;
///
/// pub enum Symbol {
///     A,
///     B,
///     C,
/// }
///
/// pub struct MyModel;
///
/// impl one_shot::Model for MyModel {
///     type Symbol = Symbol;
///     type ValueError = !;
///
///     fn probability(&self, symbol: &Self::Symbol) -> Result<Range<u32>, !> {
///         Ok(match symbol {
///             Symbol::A => 0..1,
///             Symbol::B => 1..2,
///             Symbol::C => 2..3,
///         })
///     }
///
///     fn symbol(&self, value: Self::B) -> Self::Symbol {
///         match value {
///             0..1 => Symbol::A,
///             1..2 => Symbol::B,
///             2..3 => Symbol::C,
///             _ => unreachable!(),
///         }
///     }
///
///     fn max_denominator(&self) -> u32 {
///         3
///     }
/// }
/// ```
pub trait Model {
    /// The type of symbol this [`Model`] describes
    type Symbol;

    /// Invalid symbol error
    type ValueError: std::error::Error;

    /// The internal representation to use for storing integers
    type B: BitStore;

    /// Given a symbol, return an interval representing the probability of that
    /// symbol occurring.
    ///
    /// This is given as a range, over the denominator given by
    /// [`Model::denominator`]. This range should in general include `EOF`,
    /// which is denoted by `None`.
    ///
    /// For example, from the set {heads, tails}, the interval representing
    /// heads could be `0..1`, and tails would be `1..2`, and `EOF` could be
    /// `2..3` (with a denominator of `3`).
    ///
    /// This is the inverse of the [`Model::symbol`] method
    ///
    /// # Errors
    ///
    /// This returns a custom error if the given symbol is not valid
    fn probability(&self, symbol: &Self::Symbol) -> Result<Range<Self::B>, Self::ValueError>;

    /// The maximum denominator used for probability ranges. See
    /// [`Model::probability`].
    ///
    /// This value is used to calculate an appropriate precision for the
    /// encoding, therefore this value must not change, and
    /// [`Model::denominator`] must never exceed it.
    fn max_denominator(&self) -> Self::B;

    /// Given a value, return the symbol whose probability range it falls in.
    ///
    /// `None` indicates `EOF`
    ///
    /// This is the inverse of the [`Model::probability`] method
    fn symbol(&self, value: Self::B) -> Self::Symbol;
}

impl<T> fixed_length::Model for T
where
    T: Model,
{
    type B = T::B;
    type Symbol = T::Symbol;
    type ValueError = T::ValueError;

    fn probability(&self, symbol: &Self::Symbol) -> Result<Range<Self::B>, Self::ValueError> {
        Model::probability(self, symbol)
    }

    fn max_denominator(&self) -> Self::B {
        self.max_denominator()
    }

    fn symbol(&self, value: Self::B) -> Self::Symbol {
        Model::symbol(self, value)
    }

    fn length(&self) -> usize {
        1
    }

    fn denominator(&self) -> Self::B {
        self.max_denominator()
    }
}
