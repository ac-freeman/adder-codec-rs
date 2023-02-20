pub mod framer;

#[cfg(feature = "opencv")]
pub mod transcoder;
pub mod utils; // Have to enable the 'transcoder' feature. Requires OpenCV to be installed.

pub extern crate aedat;

extern crate core;
pub extern crate davis_edi_rs;
