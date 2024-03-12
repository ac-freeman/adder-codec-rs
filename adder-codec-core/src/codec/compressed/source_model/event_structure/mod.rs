/// An `EventAdu` has many `EventCube`s
pub mod event_adu;

/// An `EventCube` has many compressed events
mod event_cube;

/// Width and height (same number) of a block
pub const BLOCK_SIZE: usize = 16;
