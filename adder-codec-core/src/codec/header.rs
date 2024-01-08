use crate::{PlaneSize, SourceCamera, TimeMode};
use serde::{Deserialize, Serialize};

pub(crate) type Magic = [u8; 5];
pub(crate) const MAGIC_RAW: Magic = [97, 100, 100, 101, 114]; // 'adder' in ASCII
pub(crate) const MAGIC_COMPRESSED: Magic = [97, 100, 100, 101, 99]; // 'addec' in ASCII

/// ADΔER event stream header
///
/// Both the raw (uncompressed) and compressed ADΔER streams have the same header structure. All
/// that changes is the [`Magic`]. A new `version` of the raw stream format necessitates a new
/// `version` of the compressed format.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub(crate) struct EventStreamHeader {
    pub(crate) magic: Magic,
    pub(crate) version: u8,
    pub(crate) endianness: u8, // 'b' = big endian
    pub(crate) width: u16,
    pub(crate) height: u16,
    pub(crate) tps: u32,
    pub(crate) ref_interval: u32,
    pub(crate) delta_t_max: u32,
    pub(crate) event_size: u8,
    pub(crate) channels: u8,
}

pub(crate) trait HeaderExtension {}
#[derive(Debug, Default, Serialize, Deserialize)]
pub(crate) struct EventStreamHeaderExtensionV0 {}
impl HeaderExtension for EventStreamHeaderExtensionV0 {}

#[derive(Debug, Default, Serialize, Deserialize)]
pub(crate) struct EventStreamHeaderExtensionV1 {
    pub(crate) source: SourceCamera,
}
impl HeaderExtension for EventStreamHeaderExtensionV1 {}

#[derive(Debug, Default, Serialize, Deserialize)]
pub(crate) struct EventStreamHeaderExtensionV2 {
    pub(crate) time_mode: TimeMode,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub(crate) struct EventStreamHeaderExtensionV3 {
    pub(crate) adu_interval: u32,
}

impl HeaderExtension for EventStreamHeaderExtensionV2 {}
impl HeaderExtension for EventStreamHeaderExtensionV3 {}

impl EventStreamHeader {
    pub(crate) fn new(
        magic: Magic,
        plane_size: PlaneSize,
        tps: u32,
        ref_interval: u32,
        delta_t_max: u32,
        codec_version: u8,
    ) -> EventStreamHeader {
        assert!(plane_size.channels > 0);
        assert!(delta_t_max > 0);
        assert!(plane_size.width > 0);
        assert!(plane_size.height > 0);
        assert!(magic == MAGIC_RAW || magic == MAGIC_COMPRESSED);

        EventStreamHeader {
            magic,
            version: codec_version,
            endianness: 98, // 'b' in ASCII, for big-endian
            width: plane_size.width,
            height: plane_size.height,
            tps,
            ref_interval,
            delta_t_max,

            // Number of bytes each event occupies
            event_size: match plane_size.channels {
                1 => 9, // If single-channel, don't need to waste 2 bytes on the c portion
                // for every event
                _ => 11,
            },
            channels: plane_size.channels,
        }
    }
}
