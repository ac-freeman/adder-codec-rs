#![warn(missing_docs)]

pub mod framer;

#[cfg(feature = "opencv")]
pub mod transcoder; // Have to enable the 'transcoder' feature. Requires OpenCV to be installed.

/// A module for utilities which may be common between programs
pub mod utils;

pub extern crate aedat;

extern crate core;
pub extern crate davis_edi_rs;
