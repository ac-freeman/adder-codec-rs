#![warn(missing_docs)]

//! # adder-codec-core
//!
//! The core types and utilities for encoding and decoding ADΔER events

/// Expose public API for encoding and decoding
pub mod codec;

pub use bitstream_io;
use bitstream_io::{BigEndian, BitReader};
use std::cmp::Ordering;
use std::fs::File;
use std::io::BufReader;
use std::ops::Add;

use thiserror::Error;

/// Error type for the `PlaneSize` struct
#[allow(missing_docs)]
#[derive(Error, Debug)]
pub enum PlaneError {
    #[error(
        "plane dimensions invalid. All must be positive. Found {width:?}, {height:?}, {channels:?}"
    )]
    InvalidPlane {
        width: u16,
        height: u16,
        channels: u8,
    },
}

#[allow(missing_docs)]
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq)]
pub enum SourceCamera {
    #[default]
    FramedU8,
    FramedU16,
    FramedU32,
    FramedU64,
    FramedF32,
    FramedF64,
    Dvs,
    DavisU8,
    Atis,
    Asint,
}

pub fn is_framed(source_camera: SourceCamera) -> bool {
    match source_camera {
        SourceCamera::FramedU8
        | SourceCamera::FramedU16
        | SourceCamera::FramedU32
        | SourceCamera::FramedU64
        | SourceCamera::FramedF32
        | SourceCamera::FramedF64 => true,
        _ => false,
    }
}

// #[cfg(feature = "compression")]
// use crate::codec::compressed::blocks::{DeltaTResidual, EventResidual};
#[cfg(feature = "compression")]
use crate::codec::compressed::stream::CompressedInput;
use crate::codec::decoder::Decoder;
use crate::codec::raw::stream::RawInput;
use crate::codec::CodecError;
use serde::{Deserialize, Serialize};

/// The type of time used in the ADΔER representation
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq)]
pub enum TimeMode {
    /// The time is the delta time from the previous event
    DeltaT,

    /// The time is the absolute time from the start of the recording
    #[default]
    AbsoluteT,

    /// TODO
    Mixed,
}

/// The size of the image plane in pixels
#[derive(Clone, Copy, Debug)]
pub struct PlaneSize {
    width: u16,
    height: u16,
    channels: u8,
}

impl Default for PlaneSize {
    fn default() -> Self {
        PlaneSize {
            width: 1,
            height: 1,
            channels: 1,
        }
    }
}

impl PlaneSize {
    /// Create a new `PlaneSize` with the given width, height, and channels
    pub fn new(width: u16, height: u16, channels: u8) -> Result<Self, PlaneError> {
        if width == 0 || height == 0 || channels == 0 {
            return Err(PlaneError::InvalidPlane {
                width,
                height,
                channels,
            });
        }
        Ok(Self {
            width,
            height,
            channels,
        })
    }
    /// The width, shorthand for `self.width`
    pub fn w(&self) -> u16 {
        self.width
    }

    /// The height, shorthand for `self.height`
    pub fn w_usize(&self) -> usize {
        self.width as usize
    }

    /// The height, shorthand for `self.height`
    pub fn h(&self) -> u16 {
        self.height
    }

    /// The height, shorthand for `self.height`
    pub fn h_usize(&self) -> usize {
        self.height as usize
    }

    /// The number of channels, shorthand for `self.channels`
    pub fn c(&self) -> u8 {
        self.channels
    }

    /// The number of channels, shorthand for `self.channels`
    pub fn c_usize(&self) -> usize {
        self.channels as usize
    }

    /// The total number of 2D pixels in the image plane, across the height and width dimension
    pub fn area_wh(&self) -> usize {
        self.width as usize * self.height as usize
    }

    /// The total number of 2D pixels in the image plane, across the width and channel dimension
    pub fn area_wc(&self) -> usize {
        self.width as usize * self.channels as usize
    }

    /// The total number of 2D pixels in the image plane, across the height and channel dimension
    pub fn area_hc(&self) -> usize {
        self.height as usize * self.channels as usize
    }

    /// The total number of 3D pixels in the image plane (2D pixels * color depth)
    pub fn volume(&self) -> usize {
        self.area_wh() * self.channels as usize
    }

    /// The smaller of the width and height dimensions
    pub fn min_resolution(&self) -> u16 {
        self.width.min(self.height)
    }

    /// The larger of the width and height dimensions
    pub fn max_resolution(&self) -> u16 {
        self.width.max(self.height)
    }
}

/// Decimation value; a pixel's sensitivity.
pub type D = u8;

/// The maximum possible [`D`] value
pub const D_MAX: D = 127;

/// Special symbol signifying no information (filler dt)
pub const D_EMPTY: D = 255;

/// Special symbol signifying no information (filler dt)
pub const D_ZERO_INTEGRATION: D = 254;

/// Special symbol signifying no [`Event`] exists
pub const D_NO_EVENT: D = 253;

#[derive(Clone, Copy, PartialEq, Default, Debug)]
pub enum Mode {
    /// Preserve temporal coherence for framed inputs. When an event fires, the ticks
    /// remaining for that input frame (and its associated intensity) are discarded. The
    /// difference in time is implied since the number of ticks per input frame is constant.
    #[default]
    FramePerfect,

    /// Do not do the above ^
    Continuous,
}

#[derive(Clone, Copy, PartialEq, Default, Debug)]
pub enum PixelMultiMode {
    Normal,

    #[default]
    Collapse,
}

/// Precision for maximum intensity representable with allowed [`D`] values
pub type UDshift = u128;

/// Array for computing the intensity to integrate for a given [`D`] value
pub const D_SHIFT: [UDshift; 128] = [
    1 << 0,
    1 << 1,
    1 << 2,
    1 << 3,
    1 << 4,
    1 << 5,
    1 << 6,
    1 << 7,
    1 << 8,
    1 << 9,
    1 << 10,
    1 << 11,
    1 << 12,
    1 << 13,
    1 << 14,
    1 << 15,
    1 << 16,
    1 << 17,
    1 << 18,
    1 << 19,
    1 << 20,
    1 << 21,
    1 << 22,
    1 << 23,
    1 << 24,
    1 << 25,
    1 << 26,
    1 << 27,
    1 << 28,
    1 << 29,
    1 << 30,
    1 << 31,
    1 << 32,
    1 << 33,
    1 << 34,
    1 << 35,
    1 << 36,
    1 << 37,
    1 << 38,
    1 << 39,
    1 << 40,
    1 << 41,
    1 << 42,
    1 << 43,
    1 << 44,
    1 << 45,
    1 << 46,
    1 << 47,
    1 << 48,
    1 << 49,
    1 << 50,
    1 << 51,
    1 << 52,
    1 << 53,
    1 << 54,
    1 << 55,
    1 << 56,
    1 << 57,
    1 << 58,
    1 << 59,
    1 << 60,
    1 << 61,
    1 << 62,
    1 << 63,
    1 << 64,
    1 << 65,
    1 << 66,
    1 << 67,
    1 << 68,
    1 << 69,
    1 << 70,
    1 << 71,
    1 << 72,
    1 << 73,
    1 << 74,
    1 << 75,
    1 << 76,
    1 << 77,
    1 << 78,
    1 << 79,
    1 << 80,
    1 << 81,
    1 << 82,
    1 << 83,
    1 << 84,
    1 << 85,
    1 << 86,
    1 << 87,
    1 << 88,
    1 << 89,
    1 << 90,
    1 << 91,
    1 << 92,
    1 << 93,
    1 << 94,
    1 << 95,
    1 << 96,
    1 << 97,
    1 << 98,
    1 << 99,
    1 << 100,
    1 << 101,
    1 << 102,
    1 << 103,
    1 << 104,
    1 << 105,
    1 << 106,
    1 << 107,
    1 << 108,
    1 << 109,
    1 << 110,
    1 << 111,
    1 << 112,
    1 << 113,
    1 << 114,
    1 << 115,
    1 << 116,
    1 << 117,
    1 << 118,
    1 << 119,
    1 << 120,
    1 << 121,
    1 << 122,
    1 << 123,
    1 << 124,
    1 << 125,
    1 << 126,
    1 << 127,
];

pub const D_SHIFT_F64: [f64; 128] = [
    (1_u128 << 0) as f64,
    (1_u128 << 1) as f64,
    (1_u128 << 2) as f64,
    (1_u128 << 3) as f64,
    (1_u128 << 4) as f64,
    (1_u128 << 5) as f64,
    (1_u128 << 6) as f64,
    (1_u128 << 7) as f64,
    (1_u128 << 8) as f64,
    (1_u128 << 9) as f64,
    (1_u128 << 10) as f64,
    (1_u128 << 11) as f64,
    (1_u128 << 12) as f64,
    (1_u128 << 13) as f64,
    (1_u128 << 14) as f64,
    (1_u128 << 15) as f64,
    (1_u128 << 16) as f64,
    (1_u128 << 17) as f64,
    (1_u128 << 18) as f64,
    (1_u128 << 19) as f64,
    (1_u128 << 20) as f64,
    (1_u128 << 21) as f64,
    (1_u128 << 22) as f64,
    (1_u128 << 23) as f64,
    (1_u128 << 24) as f64,
    (1_u128 << 25) as f64,
    (1_u128 << 26) as f64,
    (1_u128 << 27) as f64,
    (1_u128 << 28) as f64,
    (1_u128 << 29) as f64,
    (1_u128 << 30) as f64,
    (1_u128 << 31) as f64,
    (1_u128 << 32) as f64,
    (1_u128 << 33) as f64,
    (1_u128 << 34) as f64,
    (1_u128 << 35) as f64,
    (1_u128 << 36) as f64,
    (1_u128 << 37) as f64,
    (1_u128 << 38) as f64,
    (1_u128 << 39) as f64,
    (1_u128 << 40) as f64,
    (1_u128 << 41) as f64,
    (1_u128 << 42) as f64,
    (1_u128 << 43) as f64,
    (1_u128 << 44) as f64,
    (1_u128 << 45) as f64,
    (1_u128 << 46) as f64,
    (1_u128 << 47) as f64,
    (1_u128 << 48) as f64,
    (1_u128 << 49) as f64,
    (1_u128 << 50) as f64,
    (1_u128 << 51) as f64,
    (1_u128 << 52) as f64,
    (1_u128 << 53) as f64,
    (1_u128 << 54) as f64,
    (1_u128 << 55) as f64,
    (1_u128 << 56) as f64,
    (1_u128 << 57) as f64,
    (1_u128 << 58) as f64,
    (1_u128 << 59) as f64,
    (1_u128 << 60) as f64,
    (1_u128 << 61) as f64,
    (1_u128 << 62) as f64,
    (1_u128 << 63) as f64,
    (1_u128 << 64) as f64,
    (1_u128 << 65) as f64,
    (1_u128 << 66) as f64,
    (1_u128 << 67) as f64,
    (1_u128 << 68) as f64,
    (1_u128 << 69) as f64,
    (1_u128 << 70) as f64,
    (1_u128 << 71) as f64,
    (1_u128 << 72) as f64,
    (1_u128 << 73) as f64,
    (1_u128 << 74) as f64,
    (1_u128 << 75) as f64,
    (1_u128 << 76) as f64,
    (1_u128 << 77) as f64,
    (1_u128 << 78) as f64,
    (1_u128 << 79) as f64,
    (1_u128 << 80) as f64,
    (1_u128 << 81) as f64,
    (1_u128 << 82) as f64,
    (1_u128 << 83) as f64,
    (1_u128 << 84) as f64,
    (1_u128 << 85) as f64,
    (1_u128 << 86) as f64,
    (1_u128 << 87) as f64,
    (1_u128 << 88) as f64,
    (1_u128 << 89) as f64,
    (1_u128 << 90) as f64,
    (1_u128 << 91) as f64,
    (1_u128 << 92) as f64,
    (1_u128 << 93) as f64,
    (1_u128 << 94) as f64,
    (1_u128 << 95) as f64,
    (1_u128 << 96) as f64,
    (1_u128 << 97) as f64,
    (1_u128 << 98) as f64,
    (1_u128 << 99) as f64,
    (1_u128 << 100) as f64,
    (1_u128 << 101) as f64,
    (1_u128 << 102) as f64,
    (1_u128 << 103) as f64,
    (1_u128 << 104) as f64,
    (1_u128 << 105) as f64,
    (1_u128 << 106) as f64,
    (1_u128 << 107) as f64,
    (1_u128 << 108) as f64,
    (1_u128 << 109) as f64,
    (1_u128 << 110) as f64,
    (1_u128 << 111) as f64,
    (1_u128 << 112) as f64,
    (1_u128 << 113) as f64,
    (1_u128 << 114) as f64,
    (1_u128 << 115) as f64,
    (1_u128 << 116) as f64,
    (1_u128 << 117) as f64,
    (1_u128 << 118) as f64,
    (1_u128 << 119) as f64,
    (1_u128 << 120) as f64,
    (1_u128 << 121) as f64,
    (1_u128 << 122) as f64,
    (1_u128 << 123) as f64,
    (1_u128 << 124) as f64,
    (1_u128 << 125) as f64,
    (1_u128 << 126) as f64,
    (1_u128 << 127) as f64,
];

pub const D_SHIFT_F32: [f32; 128] = [
    (1_u128 << 0) as f32,
    (1_u128 << 1) as f32,
    (1_u128 << 2) as f32,
    (1_u128 << 3) as f32,
    (1_u128 << 4) as f32,
    (1_u128 << 5) as f32,
    (1_u128 << 6) as f32,
    (1_u128 << 7) as f32,
    (1_u128 << 8) as f32,
    (1_u128 << 9) as f32,
    (1_u128 << 10) as f32,
    (1_u128 << 11) as f32,
    (1_u128 << 12) as f32,
    (1_u128 << 13) as f32,
    (1_u128 << 14) as f32,
    (1_u128 << 15) as f32,
    (1_u128 << 16) as f32,
    (1_u128 << 17) as f32,
    (1_u128 << 18) as f32,
    (1_u128 << 19) as f32,
    (1_u128 << 20) as f32,
    (1_u128 << 21) as f32,
    (1_u128 << 22) as f32,
    (1_u128 << 23) as f32,
    (1_u128 << 24) as f32,
    (1_u128 << 25) as f32,
    (1_u128 << 26) as f32,
    (1_u128 << 27) as f32,
    (1_u128 << 28) as f32,
    (1_u128 << 29) as f32,
    (1_u128 << 30) as f32,
    (1_u128 << 31) as f32,
    (1_u128 << 32) as f32,
    (1_u128 << 33) as f32,
    (1_u128 << 34) as f32,
    (1_u128 << 35) as f32,
    (1_u128 << 36) as f32,
    (1_u128 << 37) as f32,
    (1_u128 << 38) as f32,
    (1_u128 << 39) as f32,
    (1_u128 << 40) as f32,
    (1_u128 << 41) as f32,
    (1_u128 << 42) as f32,
    (1_u128 << 43) as f32,
    (1_u128 << 44) as f32,
    (1_u128 << 45) as f32,
    (1_u128 << 46) as f32,
    (1_u128 << 47) as f32,
    (1_u128 << 48) as f32,
    (1_u128 << 49) as f32,
    (1_u128 << 50) as f32,
    (1_u128 << 51) as f32,
    (1_u128 << 52) as f32,
    (1_u128 << 53) as f32,
    (1_u128 << 54) as f32,
    (1_u128 << 55) as f32,
    (1_u128 << 56) as f32,
    (1_u128 << 57) as f32,
    (1_u128 << 58) as f32,
    (1_u128 << 59) as f32,
    (1_u128 << 60) as f32,
    (1_u128 << 61) as f32,
    (1_u128 << 62) as f32,
    (1_u128 << 63) as f32,
    (1_u128 << 64) as f32,
    (1_u128 << 65) as f32,
    (1_u128 << 66) as f32,
    (1_u128 << 67) as f32,
    (1_u128 << 68) as f32,
    (1_u128 << 69) as f32,
    (1_u128 << 70) as f32,
    (1_u128 << 71) as f32,
    (1_u128 << 72) as f32,
    (1_u128 << 73) as f32,
    (1_u128 << 74) as f32,
    (1_u128 << 75) as f32,
    (1_u128 << 76) as f32,
    (1_u128 << 77) as f32,
    (1_u128 << 78) as f32,
    (1_u128 << 79) as f32,
    (1_u128 << 80) as f32,
    (1_u128 << 81) as f32,
    (1_u128 << 82) as f32,
    (1_u128 << 83) as f32,
    (1_u128 << 84) as f32,
    (1_u128 << 85) as f32,
    (1_u128 << 86) as f32,
    (1_u128 << 87) as f32,
    (1_u128 << 88) as f32,
    (1_u128 << 89) as f32,
    (1_u128 << 90) as f32,
    (1_u128 << 91) as f32,
    (1_u128 << 92) as f32,
    (1_u128 << 93) as f32,
    (1_u128 << 94) as f32,
    (1_u128 << 95) as f32,
    (1_u128 << 96) as f32,
    (1_u128 << 97) as f32,
    (1_u128 << 98) as f32,
    (1_u128 << 99) as f32,
    (1_u128 << 100) as f32,
    (1_u128 << 101) as f32,
    (1_u128 << 102) as f32,
    (1_u128 << 103) as f32,
    (1_u128 << 104) as f32,
    (1_u128 << 105) as f32,
    (1_u128 << 106) as f32,
    (1_u128 << 107) as f32,
    (1_u128 << 108) as f32,
    (1_u128 << 109) as f32,
    (1_u128 << 110) as f32,
    (1_u128 << 111) as f32,
    (1_u128 << 112) as f32,
    (1_u128 << 113) as f32,
    (1_u128 << 114) as f32,
    (1_u128 << 115) as f32,
    (1_u128 << 116) as f32,
    (1_u128 << 117) as f32,
    (1_u128 << 118) as f32,
    (1_u128 << 119) as f32,
    (1_u128 << 120) as f32,
    (1_u128 << 121) as f32,
    (1_u128 << 122) as f32,
    (1_u128 << 123) as f32,
    (1_u128 << 124) as f32,
    (1_u128 << 125) as f32,
    (1_u128 << 126) as f32,
    (1_u128 << 127) as f32,
];

/// The maximum intensity representation for input data. Currently 255 for 8-bit framed input.
pub const MAX_INTENSITY: f32 = 255.0; // TODO: make variable, dependent on input bit depth

/// The default [`D`] value for every pixel at the beginning of transcode
pub const D_START: D = 7;

/// Number of ticks elapsed since a given pixel last fired an [`Event`]
pub type DeltaT = u32;

/// Absolute firing time (in ticks) of an event. For a given pixel, this will always
/// be grater than or equal to that of the pixel's last fired event.
pub type AbsoluteT = u32;

/// Large count of ticks (e.g., for tracking the running timestamp of a sequence of [Events](Event)
pub type BigT = u64;

/// Measure of an amount of light intensity
pub type Intensity = f64;

/// Pixel x- or y- coordinate address in the ADΔER model
pub type PixelAddress = u16;

/// Special pixel address when signifying the end of a sequence of [Events](Event)
pub const EOF_PX_ADDRESS: PixelAddress = u16::MAX;

/// Pixel channel address in the ADΔER model
#[repr(packed)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Coord {
    /// Pixel x-coordinate
    pub x: PixelAddress,

    /// Pixel y-coordinate
    pub y: PixelAddress,

    /// Pixel channel, if present
    pub c: Option<u8>,
}

impl Default for Coord {
    fn default() -> Self {
        Self {
            x: 0,
            y: 0,
            c: Some(0),
        }
    }
}

impl Coord {
    /// Creates a new coordinate with the given x, y, and channel
    pub fn new(x: PixelAddress, y: PixelAddress, c: Option<u8>) -> Self {
        Self { x, y, c }
    }

    /// Creates a new 2D coordinate
    pub fn new_2d(x: PixelAddress, y: PixelAddress) -> Self {
        Self { x, y, c: None }
    }

    /// Creates a new 3D coordinate with the given channel
    pub fn new_3d(x: PixelAddress, y: PixelAddress, c: u8) -> Self {
        Self { x, y, c: Some(c) }
    }

    /// Returns the x coordinate as a [`PixelAddress`]
    pub fn x(&self) -> PixelAddress {
        self.x
    }

    /// Returns the y coordinate as a [`PixelAddress`]
    pub fn y(&self) -> PixelAddress {
        self.y
    }

    /// Returns the channel as an `Option<u8>`
    pub fn c(&self) -> Option<u8> {
        self.c
    }

    /// Returns the x coordinate as a `usize`
    pub fn x_usize(&self) -> usize {
        self.x as usize
    }

    /// Returns the y coordinate as a `usize`
    pub fn y_usize(&self) -> usize {
        self.y as usize
    }

    /// Returns the channel as a usize, or 0 if the coordinate is 2D
    pub fn c_usize(&self) -> usize {
        self.c.unwrap_or(0) as usize
    }

    /// Returns true if the coordinate is 2D
    pub fn is_2d(&self) -> bool {
        self.c.is_none()
    }

    /// Returns true if the coordinate is 3D
    pub fn is_3d(&self) -> bool {
        self.c.is_some()
    }

    /// Returns true if the coordinate is valid
    pub fn is_valid(&self) -> bool {
        self.x != EOF_PX_ADDRESS && self.y != EOF_PX_ADDRESS
    }

    /// Returns true if the coordinate is the EOF coordinate
    pub fn is_eof(&self) -> bool {
        self.x == EOF_PX_ADDRESS && self.y == EOF_PX_ADDRESS
    }

    /// Is this coordinate at the border of the image?
    pub fn is_border(&self, width: usize, height: usize, cs: usize) -> bool {
        self.x_usize() < cs
            || self.x_usize() >= width - cs
            || self.y_usize() < cs
            || self.y_usize() >= height - cs
    }
}

/// A 2D coordinate representation
#[allow(missing_docs)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct CoordSingle {
    pub x: PixelAddress,
    pub y: PixelAddress,
}

/// An ADΔER event representation
#[allow(missing_docs)]
#[repr(packed)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default, Hash, Serialize, Deserialize)]
pub struct Event {
    pub coord: Coord,
    pub d: D,
    pub t: AbsoluteT,
}

#[allow(missing_docs)]
#[repr(packed)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default, Hash, Serialize, Deserialize)]
pub struct EventRelative {
    pub coord: Coord,
    pub d: D,
    pub delta_t: DeltaT,
}

/// An ADΔER event representation, without the channel component
#[allow(missing_docs)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct EventSingle {
    pub coord: CoordSingle,
    pub d: D,
    pub t: DeltaT,
}

impl From<&Event> for EventSingle {
    fn from(event: &Event) -> Self {
        EventSingle {
            coord: CoordSingle {
                x: event.coord.x,
                y: event.coord.y,
            },
            d: event.d,
            t: event.t,
        }
    }
}

impl From<EventSingle> for Event {
    fn from(event: EventSingle) -> Self {
        Event {
            coord: Coord {
                x: event.coord.x,
                y: event.coord.y,
                c: None,
            },
            d: event.d,
            t: event.t,
        }
    }
}

impl Ord for Event {
    fn cmp(&self, other: &Self) -> Ordering {
        let b = other.t;
        let a = self.t;
        b.cmp(&a)
    }
}

impl PartialOrd for Event {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// The type of data source representation
#[allow(missing_docs)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum SourceType {
    U8,
    U16,
    U32,
    U64,
    F32,
    F64,
}

const EOF_EVENT: Event = Event {
    coord: Coord {
        x: EOF_PX_ADDRESS,
        y: EOF_PX_ADDRESS,
        c: Some(0),
    },
    d: 0,
    t: 0,
};

/// Helper function for opening a file as a raw or compressed input ADΔER stream
pub fn open_file_decoder(
    file_path: &str,
) -> Result<
    (
        Decoder<BufReader<File>>,
        BitReader<BufReader<File>, BigEndian>,
    ),
    CodecError,
> {
    let mut bufreader = BufReader::new(File::open(file_path)?);
    let compression = RawInput::new();
    let mut bitreader = BitReader::endian(bufreader, BigEndian);

    // First try opening the file as a raw file, then try as a compressed file
    let stream = match Decoder::new_raw(compression, &mut bitreader) {
        Ok(reader) => reader,
        Err(CodecError::WrongMagic) => {
            #[cfg(feature = "compression")]
            {
                bufreader = BufReader::new(File::open(file_path)?);
                let compression = CompressedInput::new(0, 0); // TODO: temporary args. Need to refactor.
                bitreader = BitReader::endian(bufreader, BigEndian);
                Decoder::new_compressed(compression, &mut bitreader)?
            }

            #[cfg(not(feature = "compression"))]
            return Err(CodecError::WrongMagic);
        }
        Err(e) => {
            return Err(e);
        }
    };
    Ok((stream, bitreader))
}

/// An ADΔER event representation
#[allow(missing_docs)]
#[derive(Debug, Copy, Clone, Default, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct EventCoordless {
    pub d: D,

    pub t: AbsoluteT,
}

#[allow(missing_docs)]
#[derive(Debug, Copy, Clone, Default, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct EventCoordlessRelative {
    pub d: D,

    pub delta_t: DeltaT,
}

impl Into<f64> for EventCoordless {
    fn into(self) -> f64 {
        panic!("Not implemented")
    }
}

impl EventCoordless {
    /// Get the t or dt value
    #[inline(always)]
    pub fn t(&self) -> AbsoluteT {
        self.t as AbsoluteT
    }
}

impl From<Event> for EventCoordless {
    fn from(event: Event) -> Self {
        Self {
            d: event.d,
            t: event.t,
        }
    }
}

impl Add<EventCoordless> for EventCoordless {
    type Output = EventCoordless;

    fn add(self, _rhs: EventCoordless) -> EventCoordless {
        todo!()
    }
}

impl num_traits::Zero for EventCoordless {
    fn zero() -> Self {
        EventCoordless { d: 0, t: 0 }
    }

    fn is_zero(&self) -> bool {
        self.d.is_zero() && self.t.is_zero()
    }
}
