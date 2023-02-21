#![warn(missing_docs)]

//! # adder-codec_old-rs
//!
//! A library for transcoding to ADΔER from a variety of video sources, both framed and asynchronous

/// Tools for reconstructing frames from events
pub mod framer;

#[cfg(feature = "opencv")]
/// Tools for transcoding video sources to ADΔER
pub mod transcoder; // Have to enable the 'transcoder' feature. Requires OpenCV to be installed.

/// A module for utilities which may be common between programs
pub mod utils;

extern crate core;
pub extern crate davis_edi_rs;
pub use davis_edi_rs::aedat;
