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

/// Is the given source camera a framed source?
pub fn is_framed(source_camera: SourceCamera) -> bool {
    matches!(
        source_camera,
        SourceCamera::FramedU8
            | SourceCamera::FramedU16
            | SourceCamera::FramedU32
            | SourceCamera::FramedU64
            | SourceCamera::FramedF32
            | SourceCamera::FramedF64
    )
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
#[derive(Clone, Copy, Debug, PartialEq)]
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
pub const D_ZERO_INTEGRATION: D = 128;

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

use seq_macro::seq;

macro_rules! make_d_shift_array {
    ($name:ident, $type:ty) => {
        seq!(N in 0..=128 {
            /// Array for computing the intensity to integrate for a given [`D`] value
            pub const $name: [$type; 129] = [
                #(
                    if N == 128 { 0 as $type } else { (1_u128 << N) as $type },
                )*
            ];
        });
    };
}

make_d_shift_array!(D_SHIFT, UDshift);
make_d_shift_array!(D_SHIFT_F64, f64);
make_d_shift_array!(D_SHIFT_F32, f32);

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
        Self {
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
        Self {
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
                dbg!("Opening as compressed");
                bufreader = BufReader::new(File::open(file_path)?);
                let compression = CompressedInput::new(0, 0, 0); // TODO: temporary args. Need to refactor.
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

impl From<EventCoordless> for f64 {
    fn from(val: EventCoordless) -> Self {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dshift_arrays() {
        assert_eq!(D_SHIFT[0], 1);
        assert_eq!(D_SHIFT_F64[0], 1.0);
        assert_eq!(D_SHIFT_F32[0], 1.0);
        assert_eq!(D_SHIFT.len(), 129);
        assert_eq!(D_SHIFT_F64.len(), 129);
        assert_eq!(D_SHIFT_F32.len(), 129);
        assert_eq!(D_SHIFT[127], 1_u128 << 127);
        assert_eq!(D_SHIFT_F64[127], D_SHIFT[127] as f64);
        assert_eq!(D_SHIFT_F32[127], D_SHIFT[127] as f32);
    }

    #[test]
    fn test_plane_size() {
        let plane_size = PlaneSize::new(1, 1, 1).unwrap();
        assert_eq!(plane_size.area_wh(), 1);
        assert_eq!(plane_size.area_wc(), 1);
        assert_eq!(plane_size.area_hc(), 1);
        assert_eq!(plane_size.volume(), 1);

        let plane_size = PlaneSize::new(2, 2, 1).unwrap();
        assert_eq!(plane_size.area_wh(), 4);
        assert_eq!(plane_size.area_wc(), 2);
        assert_eq!(plane_size.area_hc(), 2);
        assert_eq!(plane_size.volume(), 4);

        let plane_size = PlaneSize::new(2, 2, 2).unwrap();
        assert_eq!(plane_size.area_wh(), 4);
        assert_eq!(plane_size.area_wc(), 4);
        assert_eq!(plane_size.area_hc(), 4);
        assert_eq!(plane_size.volume(), 8);
    }

    #[test]
    fn test_coord() {
        let coord = Coord::new(1, 2, Some(3));
        assert_eq!(coord.x(), 1);
        assert_eq!(coord.y(), 2);
        assert_eq!(coord.c(), Some(3));
        assert_eq!(coord.x_usize(), 1);
        assert_eq!(coord.y_usize(), 2);
        assert_eq!(coord.c_usize(), 3);
        assert!(coord.is_3d());
        assert!(!coord.is_2d());
        assert!(coord.is_valid());
        assert!(!coord.is_eof());

        let coord = Coord::new(1, 2, None);
        assert_eq!(coord.x(), 1);
        assert_eq!(coord.y(), 2);
        assert_eq!(coord.c(), None);
        assert_eq!(coord.x_usize(), 1);
        assert_eq!(coord.y_usize(), 2);
        assert_eq!(coord.c_usize(), 0);
        assert!(!coord.is_3d());
        assert!(coord.is_2d());
        assert!(coord.is_valid());
        assert!(!coord.is_eof());

        let coord = Coord::new(EOF_PX_ADDRESS, EOF_PX_ADDRESS, None);
        assert_eq!(coord.x(), EOF_PX_ADDRESS);
        assert_eq!(coord.y(), EOF_PX_ADDRESS);
        assert_eq!(coord.c(), None);
        assert_eq!(coord.x_usize(), EOF_PX_ADDRESS as usize);
        assert_eq!(coord.y_usize(), EOF_PX_ADDRESS as usize);
        assert_eq!(coord.c_usize(), 0);
        assert!(!coord.is_3d());
        assert!(coord.is_2d());
        assert!(!coord.is_valid());
        assert!(coord.is_eof());

        let coord = Coord::new(EOF_PX_ADDRESS, EOF_PX_ADDRESS, Some(0));
        assert_eq!(coord.x(), EOF_PX_ADDRESS);
        assert_eq!(coord.y(), EOF_PX_ADDRESS);
        assert_eq!(coord.c(), Some(0));
        assert_eq!(coord.x_usize(), EOF_PX_ADDRESS as usize);
        assert_eq!(coord.y_usize(), EOF_PX_ADDRESS as usize);
        assert_eq!(coord.c_usize(), 0);
        assert!(coord.is_3d());
        assert!(!coord.is_2d());
        assert!(!coord.is_valid());
        assert!(coord.is_eof());

        let coord = Coord::new(EOF_PX_ADDRESS, EOF_PX_ADDRESS, Some(1));
        assert_eq!(coord.x(), EOF_PX_ADDRESS);
        assert_eq!(coord.y(), EOF_PX_ADDRESS);
        assert_eq!(coord.c(), Some(1));
        assert_eq!(coord.x_usize(), EOF_PX_ADDRESS as usize);
        assert_eq!(coord.y_usize(), EOF_PX_ADDRESS as usize);
        assert_eq!(coord.c_usize(), 1);
        assert!(coord.is_3d());
        assert!(!coord.is_2d());
        assert!(!coord.is_valid());
        assert!(coord.is_eof());
    }
}
