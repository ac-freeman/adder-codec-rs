/// An `EventAdu` has many `EventCube`s
pub mod event_adu;

/// An `EventCube` has many compressed events
mod event_cube;

pub const BLOCK_SIZE: usize = 16;
pub const BLOCK_SIZE_AREA: usize = BLOCK_SIZE * BLOCK_SIZE;
