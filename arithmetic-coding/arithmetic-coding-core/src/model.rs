use std::{error::Error, ops::Range};

use crate::BitStore;

pub mod fixed_length;
pub mod max_length;
pub mod one_shot;

/// A [`Model`] is used to calculate the probability of a given symbol occuring
/// in a sequence. The [`Model`] is used both for encoding and decoding.
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
/// # use arithmetic_coding_core::Model;
///
/// pub enum Symbol {
///     A,
///     B,
///     C,
/// }
///
/// pub struct MyModel;
///
/// impl Model for MyModel {
///     type Symbol = Symbol;
///     type ValueError = !;
///
///     fn probability(&self, symbol: Option<&Self::Symbol>) -> Result<Range<u32>, !> {
///         Ok(match symbol {
///             None => 0..1,
///             Some(&Symbol::A) => 1..2,
///             Some(&Symbol::B) => 2..3,
///             Some(&Symbol::C) => 3..4,
///         })
///     }
///
///     fn symbol(&self, value: Self::B) -> Option<Self::Symbol> {
///         match value {
///             0..1 => None,
///             1..2 => Some(Symbol::A),
///             2..3 => Some(Symbol::B),
///             3..4 => Some(Symbol::C),
///             _ => unreachable!(),
///         }
///     }
///
///     fn max_denominator(&self) -> u32 {
///         4
///     }
/// }
/// ```
pub trait Model {
    /// The type of symbol this [`Model`] describes
    type Symbol;

    /// Invalid symbol error
    type ValueError: Error;

    /// The internal representation to use for storing integers
    type B: BitStore = u32;

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
    fn probability(
        &self,
        symbol: Option<&Self::Symbol>,
    ) -> Result<Range<Self::B>, Self::ValueError>;

    /// The denominator for probability ranges. See [`Model::probability`].
    ///
    /// By default this method simply returns the [`Model::max_denominator`],
    /// which is suitable for non-adaptive models.
    ///
    /// In adaptive models this value may change, however it should never exceed
    /// [`Model::max_denominator`], or it becomes possible for the
    /// [`Encoder`](crate::Encoder) and [`Decoder`](crate::Decoder) to panic due
    /// to overflow or underflow.
    fn denominator(&self) -> Self::B {
        self.max_denominator()
    }

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
    fn symbol(&self, value: Self::B) -> Option<Self::Symbol>;

    /// Update the current state of the model with the latest symbol.
    ///
    /// This method only needs to be implemented for 'adaptive' models. It's a
    /// no-op by default.
    fn update(&mut self, _symbol: Option<&Self::Symbol>) {}
}
