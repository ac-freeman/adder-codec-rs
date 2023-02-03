use crate::framer::driver::{EventCoordless, SourceType};
use crate::header::EventStreamHeader;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::fmt::Formatter;

pub mod codec;
pub mod framer;
mod header;

#[cfg(feature = "opencv")]
pub mod transcoder;
pub mod utils; // Have to enable the 'transcoder' feature. Requires OpenCV to be installed.

/// Decimation value; a pixel's sensitivity.
pub type D = u8;

/// The maximum possible [`D`] value
pub const D_MAX: D = 127;

/// Special symbol signifying no information (filler dt)
pub const D_EMPTY: D = 255;

/// Special symbol signifying no information (filler dt)
pub const D_ZERO_INTEGRATION: D = 254;

/// Special symbol signifying no event exists
pub const D_NO_EVENT: D = 253;

pub type UDshift = u128;

/// Array for computing the intensity to integrate for a given D
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
// pub const D_SHIFT: [u32; 9] = [1, 2, 4, 8, 16, 32, 64, 128, 256];

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq)]
pub enum TimeMode {
    #[default]
    DeltaT,
    AbsoluteT,
    Mixed,
}

impl fmt::Display for TimeMode {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::fmt::Display for SourceCamera {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let text = match self {
            SourceCamera::FramedU8 => {
                "FramedU8 - Framed video with 8-bit pixel depth, unsigned integer"
            }
            SourceCamera::FramedU16 => {
                "FramedU16 - Framed video with 16-bit pixel depth, unsigned integer"
            }
            SourceCamera::FramedU32 => {
                "FramedU32 - Framed video with 32-bit pixel depth, unsigned integer"
            }
            SourceCamera::FramedU64 => {
                "FramedU64 - Framed video with 64-bit pixel depth, unsigned integer"
            }
            SourceCamera::FramedF32 => {
                "FramedF32 - Framed video with 32-bit pixel depth, floating point"
            }
            SourceCamera::FramedF64 => {
                "FramedU8 - Framed camera with 64-bit pixel depth, floating point"
            }
            SourceCamera::Dvs => {
                "Dvs - Dynamic Vision System camera"
            }
            SourceCamera::DavisU8 => {
                "DavisU8 - Dynamic and Active Vision System camera. Active frames with 8-bit pixel depth, unsigned integer "
            }
            SourceCamera::Atis => {
                "Atis - Asynchronous Time-Based Image Sensor camera"
            }
            SourceCamera::Asint => {
                "Asint - Asynchronous Integration camera"
            }
        };
        write!(f, "{text}")
    }
}

/// The maximum intensity representation for input data. Currently 255 for 8-bit framed input.
pub const MAX_INTENSITY: f32 = 255.0; // TODO: make variable, dependent on input bit depth

/// The default [`D`] value for every pixel at the beginning of transcode
pub const D_START: D = 7;

/// Number of ticks elapsed since a given pixel last fired an [`pixel::Event`]
pub type DeltaT = u32;

/// Large count of ticks (e.g., for tracking the running timestamp of a sequence of events)
pub type BigT = u64;

/// Measure of an amount of light intensity
pub type Intensity = f64;

/// Pixel x- or y- coordinate address in the ADΔER model
pub type PixelAddress = u16;

pub const EOF_PX_ADDRESS: PixelAddress = u16::MAX;

pub extern crate aedat;

extern crate core;
pub extern crate davis_edi_rs;

/// The size of the image plane in pixels
#[derive(Clone)]
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
    pub fn new(width: u16, height: u16, channels: u8) -> Result<Self, String> {
        if width == 0 || height == 0 || channels == 0 {
            return Err("PlaneSize must have positive width, height, and channels".to_string());
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
}

#[repr(packed)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Coord {
    pub x: PixelAddress,
    pub y: PixelAddress,
    pub c: Option<u8>,
}

impl Coord {
    pub fn new(x: PixelAddress, y: PixelAddress, c: Option<u8>) -> Self {
        Self { x, y, c }
    }
    pub fn new_2d(x: PixelAddress, y: PixelAddress) -> Self {
        Self { x, y, c: None }
    }
    pub fn new_3d(x: PixelAddress, y: PixelAddress, c: u8) -> Self {
        Self { x, y, c: Some(c) }
    }
    pub fn x(&self) -> PixelAddress {
        self.x
    }
    pub fn y(&self) -> PixelAddress {
        self.y
    }
    pub fn c(&self) -> Option<u8> {
        self.c
    }
    pub fn x_usize(&self) -> usize {
        self.x as usize
    }
    pub fn y_usize(&self) -> usize {
        self.y as usize
    }
    pub fn c_usize(&self) -> usize {
        self.c.unwrap_or(0) as usize
    }

    pub fn is_2d(&self) -> bool {
        self.c.is_none()
    }
    pub fn is_3d(&self) -> bool {
        self.c.is_some()
    }
    pub fn is_valid(&self) -> bool {
        self.x != EOF_PX_ADDRESS && self.y != EOF_PX_ADDRESS
    }
    pub fn is_eof(&self) -> bool {
        self.x == EOF_PX_ADDRESS && self.y == EOF_PX_ADDRESS
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct CoordSingle {
    pub x: PixelAddress,
    pub y: PixelAddress,
}

/// An ADΔER event representation
#[repr(packed)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Event {
    pub coord: Coord,
    pub d: D,
    pub delta_t: DeltaT,
}

/// An ADΔER event representation, without the channel component
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct EventSingle {
    pub coord: CoordSingle,
    pub d: D,
    pub delta_t: DeltaT,
}

impl From<&Event> for EventSingle {
    fn from(event: &Event) -> Self {
        EventSingle {
            coord: CoordSingle {
                x: event.coord.x,
                y: event.coord.y,
            },
            d: event.d,
            delta_t: event.delta_t,
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
            delta_t: event.delta_t,
        }
    }
}

#[cfg(test)]
mod tests {
    // use crate::EventStreamHeader;
    // use crate::header::MAGIC_RAW;

    #[test]
    fn encode_raw() {}
}
//
