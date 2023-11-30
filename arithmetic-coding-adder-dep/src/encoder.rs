//! The [`Encoder`] half of the arithmetic coding library.

use std::{io, ops::Range};
use std::marker::PhantomData;

use bitstream_io::BitWrite;

use crate::{BitStore, Error, Model};
use crate::Error::ValueError;

// this algorithm is derived from this article - https://marknelson.us/posts/2014/10/19/data-compression-with-arithmetic-coding.html

/// An arithmetic encoder
///
/// An arithmetic decoder converts a stream of symbols into a stream of bits,
/// using a predictive [`Model`].
#[derive(Debug)]
pub struct Encoder< M, W>
where
    M: Model,
    W: BitWrite,
{
    /// The model used for the encoder
    pub model: M,
    state: State< M::B, W>,
}

impl< M, W> Encoder< M, W>
where
    M: Model,
    W: BitWrite,
{
    /// Construct a new [`Encoder`].
    ///
    /// The 'precision' of the encoder is maximised, based on the number of bits
    /// needed to represent the [`Model::denominator`]. 'precision' bits is
    /// equal to [`BitStore::BITS`] - [`Model::denominator`] bits. If you need
    /// to set the precision manually, use [`Encoder::with_precision`].
    ///
    /// # Panics
    ///
    /// The calculation of the number of bits used for 'precision' is subject to
    /// the following constraints:
    ///
    /// - The total available bits is [`BitStore::BITS`]
    /// - The precision must use at least 2 more bits than that needed to
    ///   represent [`Model::denominator`]
    ///
    /// If these constraints cannot be satisfied this method will panic in debug
    /// builds
    pub fn new(model: M) -> Self {
        let frequency_bits = model.max_denominator().log2() + 1;
        let precision = M::B::BITS - frequency_bits;
        Self::with_precision(model, precision)
    }

    /// Construct a new [`Encoder`] with a custom precision.
    ///
    /// # Panics
    ///
    /// The calculation of the number of bits used for 'precision' is subject to
    /// the following constraints:
    ///
    /// - The total available bits is [`BitStore::BITS`]
    /// - The precision must use at least 2 more bits than that needed to
    ///   represent [`Model::denominator`]
    ///
    /// If these constraints cannot be satisfied this method will panic in debug
    /// builds
    pub fn with_precision(model: M, precision: u32) -> Self {
        let frequency_bits = model.max_denominator().log2() + 1;
        debug_assert!(
            (precision >= (frequency_bits + 2)),
            "not enough bits of precision to prevent overflow/underflow",
        );
        debug_assert!(
            (frequency_bits + precision) <= M::B::BITS,
            "not enough bits in BitStore to support the required precision",
        );

        Self {
            model,
            state: State::new(precision),
        }
    }

    /// todo
    pub const fn with_state(state: State< M::B, W>, model: M) -> Self {
        Self { model, state }
    }

    /// Encode a stream of symbols into the provided output.
    ///
    /// This method will encode all the symbols in the iterator, followed by EOF
    /// (`None`), and then call [`Encoder::flush`].
    ///
    /// # Errors
    ///
    /// This method can fail if the underlying [`BitWrite`] cannot be written
    /// to.
    pub fn encode_all(
        &mut self,
        symbols: impl IntoIterator<Item = M::Symbol>,
        output: &mut W,
    ) -> Result<(), Error> {
        for symbol in symbols {
            self.encode(Some(&symbol), output)?;
        }
        self.encode(None, output)?;
        self.flush(output)?;
        Ok(())
    }

    /// Encode a symbol into the provided output.
    ///
    /// When you finish encoding symbols, you must manually encode an EOF symbol
    /// by calling [`Encoder::encode`] with `None`.
    ///
    /// The internal buffer must be manually flushed using [`Encoder::flush`].
    ///
    /// # Errors
    ///
    /// This method can fail if the underlying [`BitWrite`] cannot be written
    /// to.
    pub fn encode(&mut self, symbol: Option<&M::Symbol>, output: &mut W) -> Result<(), Error> {
        let p = match self.model.probability(symbol) {
            Ok(p) => {p}
            Err(_) => {return Err(ValueError)}
        } ;
        let denominator = self.model.denominator();
        debug_assert!(
            denominator <= self.model.max_denominator(),
            "denominator is greater than maximum!"
        );

        self.state.scale(p, denominator, output)?;
        self.model.update(symbol);

        Ok(())
    }

    /// Flush any pending bits from the buffer
    ///
    /// This method must be called when you finish writing symbols to a stream
    /// of bits. This is called automatically when you use
    /// [`Encoder::encode_all`].
    ///
    /// # Errors
    ///
    /// This method can fail if the underlying [`BitWrite`] cannot be written
    /// to.
    pub fn flush(&mut self, output: &mut W) -> io::Result<()> {
        self.state.flush(output)
    }

    /// todo
    pub fn into_inner(self) -> (M, State<M::B, W>) {
        (self.model, self.state)
    }

    /// Reuse the internal state of the Encoder with a new model.
    ///
    /// Allows for chaining multiple sequences of symbols into a single stream
    /// of bits
    pub fn chain<X>(self, model: X) -> Encoder< X, W>
    where
        X: Model<B = M::B>,
    {
        Encoder {
            model,
            state: self.state,
        }
    }
}

/// A convenience struct which stores the internal state of an [`Encoder`].
#[derive(Debug)]
pub struct State< B, W>
where
    B: BitStore,
    W: BitWrite,
{
    precision: u32,
    low: B,
    high: B,
    pending: u32,
    _marker: PhantomData<W>,
}

impl<B, W> State< B, W>
where
    B: BitStore,
    W: BitWrite,
{
    /// todo
    #[must_use] pub fn new(precision: u32) -> Self {
        let low = B::ZERO;
        let high = B::ONE << precision;
        let pending = 0;

        Self {
            precision,
            low,
            high,
            pending,
            _marker: PhantomData,
        }
    }

    fn three_quarter(&self) -> B {
        self.half() + self.quarter()
    }

    fn half(&self) -> B {
        B::ONE << (self.precision - 1)
    }

    fn quarter(&self) -> B {
        B::ONE << (self.precision - 2)
    }

    fn scale(&mut self, p: Range<B>, denominator: B, output: &mut W) -> io::Result<()> {
        let range = self.high - self.low + B::ONE;

        self.high = self.low + (range * p.end) / denominator - B::ONE;
        self.low += (range * p.start) / denominator;

        self.normalise(output)
    }

    fn normalise(&mut self, output: &mut W) -> io::Result<()> {
        while self.high < self.half() || self.low >= self.half() {
            if self.high < self.half() {
                self.emit(false, output)?;
                self.high <<= 1;
                self.low <<= 1;
            } else {
                // self.low >= self.half()
                self.emit(true, output)?;
                self.low = (self.low - self.half()) << 1;
                self.high = (self.high - self.half()) << 1;
            }
        }

        while self.low >= self.quarter() && self.high < (self.three_quarter()) {
            self.pending += 1;
            self.low = (self.low - self.quarter()) << 1;
            self.high = (self.high - self.quarter()) << 1;
        }

        Ok(())
    }

    fn emit(&mut self, bit: bool, output: &mut W) -> io::Result<()> {
        output.write_bit(bit)?;
        for _ in 0..self.pending {
            output.write_bit(!bit)?;
        }
        self.pending = 0;
        Ok(())
    }

    /// todo
    pub fn flush(&mut self, output: &mut W) -> io::Result<()> {
        self.pending += 1;
        if self.low <= self.quarter() {
            self.emit(false, output)?;
        } else {
            self.emit(true, output)?;
        }

        Ok(())
    }
}
