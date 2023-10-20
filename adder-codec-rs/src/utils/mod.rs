/// A module for simultaneously transcoding a video source to ADΔER and reconstructing a framed
/// video from ADΔER
pub mod simulproc;

/// A module for migrating streams from one format to another
pub mod stream_migration;

/// Computer vision utilities
pub mod cv;

#[cfg(feature = "feature-logging")]
pub mod logging;
/// A module for visualizing streams
pub mod viz;
