use serde::{Deserialize, Serialize};

/// What data structure does the AVU contain?
#[derive(Default, Serialize, Deserialize, Clone, Copy)]
pub enum Type {
    #[default]
    AbsEvents,
    DtEvents,
}

/// AVU header. If receiver has scrubbed to an arbitrary point in the stream, then the decoder can
/// pick back up if this is `avu_type == AbsEvents`.
#[repr(packed)]
#[derive(Default, Serialize, Deserialize)]
pub(crate) struct AvuHeader {
    /// The absolute time of the first event in the AVU
    pub(crate) time: u64,

    pub(crate) avu_type: Type,

    /// Various flags for the decoder
    pub(crate) flags: u32,

    /// The size of the AVU payload, in bytes
    pub(crate) size: u64,
}

/// Asynchronous Video Unit
#[derive(Default, Serialize, Deserialize)]
pub(crate) struct Avu {
    pub(crate) header: AvuHeader,
    pub(crate) data: Vec<u8>,
}
