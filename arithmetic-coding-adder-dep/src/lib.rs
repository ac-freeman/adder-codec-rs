//! Arithmetic coding library

#![deny(
    missing_docs,
    clippy::all,
    missing_debug_implementations,
    clippy::cargo
)]
#![warn(clippy::pedantic)]

pub use arithmetic_coding_core_adder_dep::{fixed_length, max_length, one_shot, BitStore, Model};

pub mod decoder;
pub mod encoder;

pub use decoder::Decoder;
pub use encoder::Encoder;

/// Errors that can occur during encoding/decoding
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Io error when reading/writing bits from a stream
    #[error("io error")]
    Io(#[from] std::io::Error),

    /// Invalid symbol
    #[error("invalid symbol")]
    ValueError,
}
