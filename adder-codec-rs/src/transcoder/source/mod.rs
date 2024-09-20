use crate::transcoder::source::video::Source;
use crate::transcoder::source::video::SourceError;
use crate::transcoder::source::video::Video;
use adder_codec_core::Event;
use enum_dispatch::enum_dispatch;
use video_rs_adder_dep::Frame;

#[cfg(feature = "open-cv")]
use crate::transcoder::source::davis::Davis;
use crate::transcoder::source::framed::Framed;
use crate::transcoder::source::prophesee::Prophesee;
use std::io::Write;

/// Tools for transcoding from a DVS/DAVIS video source to ADΔER
#[cfg(feature = "open-cv")]
pub mod davis;

/// Tools for transcoding from a framed video source to ADΔER
pub mod framed;

/// Common functions and structs for all transcoder sources
pub mod video;

/// Tools for transcoding from a Prophesee video source to ADΔER
pub mod prophesee;

#[enum_dispatch(Source<W>)]
pub enum AdderSource<W: Write + 'static + std::marker::Send + std::marker::Sync> {
    Framed(Framed<W>),
    #[cfg(feature = "open-cv")]
    Davis(Davis<W>),
    Prophesee(Prophesee<W>),
}
