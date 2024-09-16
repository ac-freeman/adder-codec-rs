//! Helper trait for creating fixed-length Models

use std::ops::Range;

use crate::BitStore;

/// A [`Model`] is used to calculate the probability of a given symbol occuring
/// in a sequence.
///
/// The [`Model`] is used both for encoding and decoding. A
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
/// # use arithmetic_coding_core::fixed_length;
///
/// pub enum Symbol {
///     A,
///     B,
///     C,
/// }
///
/// pub struct MyModel;
///
/// impl fixed_length::Model for MyModel {
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
///
///     fn length(&self) -> usize {
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
    fn probability(&self, symbol: &Self::Symbol) -> Result<Range<Self::B>, Self::ValueError>;

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
    fn symbol(&self, value: Self::B) -> Self::Symbol;

    /// Update the current state of the model with the latest symbol.
    ///
    /// This method only needs to be implemented for 'adaptive' models. It's a
    /// no-op by default.
    fn update(&mut self, _symbol: &Self::Symbol) {}

    /// The total number of sumbols to encode
    fn length(&self) -> usize;
}

/// A wrapper which converts a [`fixed_length::Model`](Model) to a
/// [`crate::Model`].
#[derive(Debug, Clone)]
pub struct Wrapper<M>
where
    M: Model,
{
    model: M,
    remaining: usize,
}

impl<M> Wrapper<M>
where
    M: Model,
{
    /// Construct a new wrapper from a [`fixed_length::Model`](Model)
    pub fn new(model: M) -> Self {
        let remaining = model.length();
        Self { model, remaining }
    }
}

impl<M> crate::Model for Wrapper<M>
where
    M: Model,
{
    type B = M::B;
    type Symbol = M::Symbol;
    type ValueError = Error<M::ValueError>;

    fn probability(
        &self,
        symbol: Option<&Self::Symbol>,
    ) -> Result<Range<Self::B>, Self::ValueError> {
        if self.remaining > 0 {
            if let Some(s) = symbol {
                // Expected a symbol and got one. return the probability.
                self.model.probability(s).map_err(Self::ValueError::Value)
            } else {
                // We are expecting more symbols, but got an EOF
                Err(Self::ValueError::UnexpectedEof)
            }
        } else if symbol.is_some() {
            // we should be finished, but got an extra symbol
            Err(Error::UnexpectedSymbol)
        } else {
            // got an EOF when we expected it, return a 100% probability
            Ok(Self::B::ZERO..self.denominator())
        }
    }

    fn max_denominator(&self) -> Self::B {
        self.model.max_denominator()
    }

    fn symbol(&self, value: Self::B) -> Option<Self::Symbol> {
        if self.remaining > 0 {
            Some(self.model.symbol(value))
        } else {
            None
        }
    }

    fn denominator(&self) -> Self::B {
        self.model.denominator()
    }

    fn update(&mut self, symbol: Option<&Self::Symbol>) {
        if let Some(s) = symbol {
            self.model.update(s);
            self.remaining -= 1;
        }
    }
}

/// Fixed-length encoding/decoding errors
#[derive(Debug, thiserror::Error)]
pub enum Error<E>
where
    E: std::error::Error,
{
    /// Model received an EOF when it expected more symbols
    #[error("Unexpected EOF")]
    UnexpectedEof,

    /// Model received a symbol when it expected an EOF
    #[error("Unexpected Symbol")]
    UnexpectedSymbol,

    /// The model received an invalid symbol
    #[error(transparent)]
    Value(E),
}
