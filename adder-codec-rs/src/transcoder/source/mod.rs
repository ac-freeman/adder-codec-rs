/// Tools for transcoding from a DVS/DAVIS video source to ADΔER
#[cfg(feature = "open-cv")]
pub mod davis;

/// Tools for transcoding from a framed video source to ADΔER
pub mod framed;

/// Common functions and structs for all transcoder sources
pub mod video;
mod prophesee;


