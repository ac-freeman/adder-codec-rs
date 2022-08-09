use std::fs::File;

pub(crate) type Magic = [u8; 5];
pub(crate) const MAGIC_RAW: Magic = ['a' as u8,'d' as u8,'d' as u8,'e' as u8,'r' as u8];
pub(crate) const MAGIC_COMPRESSED: Magic = ['a' as u8,'d' as u8,'d' as u8,'e' as u8,'c' as u8];

/// Both the raw (uncompressed) and compressed ADDER streams have the same header structure. All
/// that changes is [magic]. A new [version] of the raw stream format necessitates a new [version]
/// of the compressed format.
#[derive(Debug, Default, Clone)]
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

use std::io::{BufReader, Read};
use bytes::{Buf, Bytes};

impl EventStreamHeader {
    pub fn new(
        magic: Magic,
        width: u16,
        height: u16,
        tps: u32,
        ref_interval: u32,
        delta_t_max: u32,
        channels: u8,
    ) -> EventStreamHeader {
        assert!(channels > 0);
        assert!(delta_t_max > 0);
        assert!(width > 0);
        assert!(height > 0);
        assert!(magic == MAGIC_RAW || magic == MAGIC_COMPRESSED);

        EventStreamHeader {
            magic,
            version: 0,
            endianness: 'b' as u8,
            width,
            height,
            tps,
            ref_interval,
            delta_t_max,

            // Number of bytes each event occupies
            event_size: match channels {
                1 => {9},   // If single-channel, don't need to waste a byte on the c portion
                            // for every event
                _ => {10}
            },
            channels,
        }
    }

    pub fn read_header(reader: &mut BufReader<File>) -> EventStreamHeader {
        let mut buf = vec![0u8; 25];
        reader.read_exact(&mut buf).unwrap();
        let mut byte_buffer = &buf[..];
        let mut header = EventStreamHeader::default();
        header.magic = [
            byte_buffer.get_u8(), byte_buffer.get_u8(), byte_buffer.get_u8(), byte_buffer.get_u8(), byte_buffer.get_u8()
            ];
        assert!(header.magic == MAGIC_RAW || header.magic == MAGIC_COMPRESSED);
        header.version = byte_buffer.get_u8();
        header.endianness = byte_buffer.get_u8();
        header.width = byte_buffer.get_u16();
        header.height = byte_buffer.get_u16();
        header.tps = byte_buffer.get_u32();
        header.ref_interval = byte_buffer.get_u32();
        header.delta_t_max = byte_buffer.get_u32();
        header.event_size = byte_buffer.get_u8();
        header.channels = byte_buffer.get_u8();

        assert!(header.channels <= 3);
        assert!(header.delta_t_max >= header.ref_interval);

        header
    }
}

impl From<&EventStreamHeader> for Bytes {
    fn from(header: &EventStreamHeader) -> Self {
        Bytes::from([
            &header.magic as &[u8],
            &header.version.to_be_bytes() as &[u8],
            &header.endianness.to_be_bytes() as &[u8],
            &header.width.to_be_bytes() as &[u8],
            &header.height.to_be_bytes() as &[u8],
            &header.tps.to_be_bytes() as &[u8],
            &header.ref_interval.to_be_bytes() as &[u8],
            &header.delta_t_max.to_be_bytes() as &[u8],
            &header.event_size.to_be_bytes() as &[u8],
            &header.channels.to_be_bytes() as &[u8],
                    ].concat()
        )
    }
}