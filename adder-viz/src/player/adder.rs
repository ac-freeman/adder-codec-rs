use adder_codec_rs::framer::driver::FramerMode::INSTANTANEOUS;
use adder_codec_rs::framer::driver::{FrameSequence, FramerBuilder};
use adder_codec_rs::raw::stream::Raw;
use adder_codec_rs::transcoder::source::video::FramedViewMode;
use adder_codec_rs::{Codec, DeltaT};
use bevy::prelude::Image;
use opencv::core::{create_continuous, Mat, CV_8UC1, CV_8UC3};
use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};

#[derive(Default)]
pub struct AdderPlayer {
    pub(crate) framer_builder: Option<FramerBuilder>,
    pub(crate) frame_sequence: Option<FrameSequence<u8>>, // TODO: remove this
    pub(crate) input_stream: Option<Raw>,
    pub(crate) current_t_ticks: DeltaT,
    pub(crate) display_mat: Mat,
    pub(crate) live_image: Image,
    pub(crate) path_buf: Option<PathBuf>,
}

unsafe impl Sync for AdderPlayer {}

#[derive(Debug)]
struct AdderPlayerError(String);

impl fmt::Display for AdderPlayerError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "ADDER player: {}", self.0)
    }
}

impl Error for AdderPlayerError {}

impl AdderPlayer {
    pub(crate) fn new(
        path_buf: &Path,
        playback_speed: f32,
        view_mode: FramedViewMode,
    ) -> Result<Self, Box<dyn Error>> {
        match path_buf.extension() {
            None => Err(Box::new(AdderPlayerError("Invalid file type".into()))),
            Some(ext) => match ext.to_str() {
                None => Err(Box::new(AdderPlayerError("Invalid file type".into()))),
                Some("adder") => {
                    let input_path = path_buf.to_str().expect("Invalid string").to_string();
                    let mut stream: Raw = Codec::new();
                    stream.open_reader(input_path).expect("Invalid path");
                    stream.decode_header().expect("Invalid header");

                    let mut reconstructed_frame_rate = (stream.tps / stream.ref_interval) as f64;

                    reconstructed_frame_rate /= playback_speed as f64;

                    let framer_builder: FramerBuilder =
                        FramerBuilder::new(stream.plane.clone(), 260)
                            .codec_version(stream.codec_version)
                            .time_parameters(
                                stream.tps,
                                stream.ref_interval,
                                stream.delta_t_max,
                                reconstructed_frame_rate,
                            )
                            .mode(INSTANTANEOUS)
                            .view_mode(view_mode)
                            .source(stream.get_source_type(), stream.source_camera);

                    let frame_sequence: FrameSequence<u8> = framer_builder.clone().finish();

                    let mut display_mat = Mat::default();
                    match stream.plane.c() {
                        1 => {
                            create_continuous(
                                stream.plane.h() as i32,
                                stream.plane.w() as i32,
                                CV_8UC1,
                                &mut display_mat,
                            )?;
                        }
                        3 => {
                            create_continuous(
                                stream.plane.h() as i32,
                                stream.plane.w() as i32,
                                CV_8UC3,
                                &mut display_mat,
                            )?;
                        }
                        _ => {
                            return Err(Box::new(AdderPlayerError(
                                "Bad number of channels".into(),
                            )));
                        }
                    }

                    Ok(AdderPlayer {
                        framer_builder: Some(framer_builder),
                        frame_sequence: Some(frame_sequence),
                        input_stream: Some(stream),
                        current_t_ticks: 0,
                        live_image: Default::default(),
                        display_mat,
                        path_buf: Some(path_buf.to_path_buf()),
                    })
                }
                Some(_) => Err(Box::new(AdderPlayerError("Invalid file type".into()))),
            },
        }
    }
}
