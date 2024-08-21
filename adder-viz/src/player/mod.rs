use adder_codec_rs::utils::viz::ShowFeatureMode;
use std::path::PathBuf;

mod adder;
pub mod ui;

/// Core parameters which require a total reset of the player. These parameters
/// cannot be adaptively changed during a player operation.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CoreParams {
    pub input_path_buf_0: Option<PathBuf>,
    pub playback_speed: f32,
}

impl Default for CoreParams {
    fn default() -> Self {
        Self {
            input_path_buf_0: None,
            playback_speed: 1.0,
        }
    }
}

/// UI-driven parameters which do not require a total reset of the player. These
/// parameters can be adaptively changed during a player operation.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct AdaptiveParams {
    pub thread_count: usize,
    pub detect_features: bool,
    pub show_features: ShowFeatureMode,
    pub buffer_limit: Option<u32>,
}

impl Default for AdaptiveParams {
    fn default() -> Self {
        Self {
            thread_count: 1,
            detect_features: false,
            show_features: ShowFeatureMode::Off,
            buffer_limit: None,
        }
    }
}
