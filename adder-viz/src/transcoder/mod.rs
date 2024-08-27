use crate::utils::PlotY;
use adder_codec_rs::adder_codec_core::codec::rate_controller::{Crf, DEFAULT_CRF_QUALITY};
use adder_codec_rs::adder_codec_core::codec::{EncoderOptions, EncoderType};
use adder_codec_rs::adder_codec_core::{PixelMultiMode, TimeMode};
use adder_codec_rs::transcoder::source::davis::TranscoderMode;
use adder_codec_rs::transcoder::source::video::FramedViewMode;
use adder_codec_rs::utils::viz::ShowFeatureMode;
use std::collections::VecDeque;
use std::path::PathBuf;
use tokio::sync::Mutex;

pub mod adder;
pub mod ui;

/// UI-driven parameters which do not require a total reset of the transcoder. These
/// parameters can be adaptively changed during a transcoder operation.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct AdaptiveParams {
    pub auto_quality: bool,
    pub crf_number: u8,
    pub encoder_options: EncoderOptions,
    pub thread_count: usize,
    pub show_original: bool,
    pub view_mode_radio_state: FramedViewMode,
    pub detect_features: bool,
    pub show_features: ShowFeatureMode,
    pub feature_rate_adjustment: bool,
    pub feature_cluster: bool,
    optimize_c: bool,
    optimize_c_frequency: u32,
}

/// Core parameters which require a total reset of the transcoder. These parameters
/// cannot be adaptively changed during a transcoder operation.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CoreParams {
    pub delta_t_ref: u32,
    pub color: bool,
    pub scale: f64,
    pub delta_t_max_mult: u32,
    pub adu_interval: u32,
    pub time_mode: TimeMode,
    pub encoder_type: EncoderType,
    pub input_path_buf_0: Option<PathBuf>,
    pub output_path: Option<PathBuf>,
    pub(crate) integration_mode_radio_state: PixelMultiMode,
    davis_mode_radio_state: TranscoderMode,
    davis_output_fps: f64,
}

/// These are not passed along to the transcoder, but are used to store settings for quality metrics
/// and other information about the transcoder's state.
#[derive(Default, Debug, Clone, PartialEq)]
pub(crate) struct InfoParams {
    pub metric_mse: bool,
    pub metric_psnr: bool,
    pub metric_ssim: bool,
}

#[derive(Debug, Clone, PartialEq, Default)]
struct EventRateMsg {
    total_events: u64,
    events_per_sec: f64,
    events_ppc_total: f64,
    events_ppc_per_sec: f64,
    transcoded_fps: f64,
    num_pixels: u64,
    running_input_bitrate: f64,
}

impl Default for AdaptiveParams {
    fn default() -> Self {
        AdaptiveParams {
            auto_quality: true,
            crf_number: DEFAULT_CRF_QUALITY,
            encoder_options: EncoderOptions {
                event_drop: Default::default(),
                event_order: Default::default(),
                crf: Crf::new(None, Default::default()),
            },
            thread_count: 1,
            show_original: false,
            view_mode_radio_state: Default::default(),
            detect_features: false,
            show_features: ShowFeatureMode::Off,
            feature_rate_adjustment: false,
            feature_cluster: false,
            optimize_c: false,
            optimize_c_frequency: 10,
        }
    }
}

impl Default for CoreParams {
    fn default() -> Self {
        CoreParams {
            delta_t_ref: 255,
            color: false,
            scale: 0.25,
            delta_t_max_mult: 30,
            adu_interval: 30,
            time_mode: Default::default(),
            encoder_type: Default::default(),
            integration_mode_radio_state: Default::default(),
            davis_mode_radio_state: TranscoderMode::RawDavis,
            input_path_buf_0: None,
            output_path: None,
            davis_output_fps: 100.0,
        }
    }
}

pub struct InfoUiState {
    //     pub events_per_sec: f64,
    //     pub events_ppc_per_sec: f64,
    //     pub events_ppc_total: f64,
    //     pub events_total: u64,
    //     pub event_size: u8,
    //     source_samples_per_sec: f64,
    //     plane: PlaneSize,
    //     pub source_name: RichText,
    //     pub output_name: OutputName,
    //     pub davis_latency: Option<f64>,
    //     pub(crate) input_path_0: Option<PathBuf>,
    //     pub(crate) input_path_1: Option<PathBuf>,
    //     pub(crate) output_path: Option<PathBuf>,
    //     plot_points_eventrate_y: PlotY,
    //     pub(crate) plot_points_raw_adder_bitrate_y: PlotY,
    //     pub(crate) plot_points_raw_source_bitrate_y: PlotY,
    error_string: Option<String>,
    plot_points_psnr_y: PlotY,
    plot_points_mse_y: PlotY,
    plot_points_ssim_y: PlotY,
    plot_points_raw_adder_bitrate_y: PlotY,
    plot_points_raw_source_bitrate_y: PlotY,
    total_events: u64,
    events_per_sec: f64,
    events_ppc_total: f64,
    events_ppc_per_sec: f64,
    transcoded_fps: f64,
    //     plot_points_latency_y: PlotY,
    //     pub view_mode_radio_state: FramedViewMode, // TODO: Move to different struct
}

impl Default for InfoUiState {
    fn default() -> Self {
        let plot_points: VecDeque<Option<f64>> = (0..1000).map(|_| None).collect();
        InfoUiState {
            error_string: None,
            plot_points_psnr_y: PlotY {
                points: plot_points.clone(),
            },
            plot_points_mse_y: PlotY {
                points: plot_points.clone(),
            },
            plot_points_ssim_y: PlotY {
                points: plot_points.clone(),
            },
            plot_points_raw_adder_bitrate_y: PlotY {
                points: plot_points.clone(),
            },
            plot_points_raw_source_bitrate_y: PlotY {
                points: plot_points.clone(),
            },
            total_events: 0,
            events_per_sec: 0.0,
            events_ppc_total: 0.0,
            events_ppc_per_sec: 0.0,
            transcoded_fps: 0.0,
        }
    }
}
