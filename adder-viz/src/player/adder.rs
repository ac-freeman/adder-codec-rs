use crate::player::ui::ReconstructionMethod;
use adder_codec_rs::codec::raw::stream::Raw;
use adder_codec_rs::codec::Codec;
use adder_codec_rs::framer::driver::FramerMode::INSTANTANEOUS;
use adder_codec_rs::framer::driver::{FrameSequence, Framer, FramerBuilder};
use adder_codec_rs::framer::scale_intensity::event_to_intensity;
use adder_codec_rs::transcoder::source::video::FramedViewMode;
use adder_codec_rs::{DeltaT, SourceCamera, D_ZERO_INTEGRATION};
use bevy::prelude::Image;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use opencv::core::{create_continuous, Mat, MatTraitConstManual, MatTraitManual, CV_8UC1, CV_8UC3};
use opencv::imgproc;
use std::error::Error;
use std::fmt;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

pub type PlayerArtifact = (u64, Option<Image>);
pub type PlayerStreamArtifact = (u64, StreamState, Option<Image>);

#[derive(Default, Copy, Clone)]
pub struct StreamState {
    pub(crate) current_t_ticks: DeltaT,
    pub(crate) tps: DeltaT,
    pub(crate) file_pos: u64,
    pub(crate) volume: usize,
}

#[derive(Default)]
pub struct AdderPlayer {
    pub(crate) framer_builder: Option<FramerBuilder>,
    pub(crate) frame_sequence: Option<FrameSequence<u8>>, // TODO: remove this
    pub(crate) input_stream: Option<Raw>,
    pub(crate) display_mat: Mat,
    playback_speed: f32,
    reconstruction_method: ReconstructionMethod,
    current_frame: u32,
    stream_state: StreamState,
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
            Some(ext) => match ext.to_ascii_lowercase().to_str() {
                None => Err(Box::new(AdderPlayerError("Invalid file type".into()))),
                Some("adder") => {
                    let input_path = path_buf.to_str().expect("Invalid string").to_string();
                    let mut stream: Raw = Codec::new();
                    let file = File::open(input_path.clone())?;
                    stream.set_input_stream(Some(BufReader::new(file)));
                    stream.decode_header()?;

                    let mut reconstructed_frame_rate = (stream.tps / stream.ref_interval) as f64;

                    reconstructed_frame_rate /= playback_speed as f64;

                    let framer_builder: FramerBuilder =
                        FramerBuilder::new(stream.plane.clone(), 260)
                            .codec_version(stream.codec_version, stream.time_mode)
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
                        stream_state: StreamState {
                            current_t_ticks: 0,
                            tps: stream.tps,
                            file_pos: 0,
                            volume: stream.plane.volume(),
                        },
                        framer_builder: Some(framer_builder),
                        frame_sequence: Some(frame_sequence),
                        input_stream: Some(stream),
                        display_mat,
                        playback_speed,
                        reconstruction_method: ReconstructionMethod::Accurate,
                        current_frame: 0,
                    })
                }
                Some(_) => Err(Box::new(AdderPlayerError("Invalid file type".into()))),
            },
        }
    }

    pub fn reconstruction_method(mut self, method: ReconstructionMethod) -> Self {
        self.reconstruction_method = method;
        self
    }

    pub fn stream_pos(mut self, pos: u64) -> Self {
        if let Some(ref mut stream) = self.input_stream {
            if pos > stream.header_size as u64 {
                match stream.set_input_stream_position(pos) {
                    Ok(_) => {}
                    Err(_) => {}
                }
            } else {
                match stream.set_input_stream_position(stream.header_size as u64) {
                    Ok(_) => {}
                    Err(_) => {}
                }
            }
        }
        self
    }

    pub fn consume_source(&mut self) -> PlayerStreamArtifact {
        let stream = match &mut self.input_stream {
            None => {
                return (0, self.stream_state, None);
            }
            Some(s) => s,
        };

        // Reset the stats if we're starting a new looped playback of the video
        if let Ok(pos) = stream.get_input_stream_position() {
            if pos == stream.header_size as u64 {
                match &mut self.frame_sequence {
                    None => { // TODO: error
                    }
                    Some(frame_sequence) => {
                        frame_sequence.state.frames_written = 0;
                    }
                };
            }
        }

        let res = match self.reconstruction_method {
            ReconstructionMethod::Fast => self.consume_source_fast(),
            ReconstructionMethod::Accurate => self.consume_source_accurate(),
        };

        self.stream_state.file_pos = match &mut self.input_stream {
            None => 0,
            Some(s) => s.get_input_stream_position().unwrap_or(0),
        };
        match res {
            Ok(a) => (a.0, self.stream_state, a.1),
            Err(_b) => (0, self.stream_state, None),
        }
    }

    fn consume_source_fast(&mut self) -> Result<PlayerArtifact, Box<dyn Error>> {
        let mut event_count = 0;

        if self.current_frame == 0 {
            self.current_frame = 1; // TODO: temporary hack
        }
        let stream = match &mut self.input_stream {
            None => {
                return Ok((0, None));
            }
            Some(s) => s,
        };

        let _frame_sequence = match &mut self.frame_sequence {
            None => {
                return Ok((0, None));
            }
            Some(s) => s,
        };

        let frame_length = stream.ref_interval as f64 * self.playback_speed as f64; //TODO: temp

        let display_mat = &mut self.display_mat;

        let image_bevy = loop {
            if self.stream_state.current_t_ticks as u128
                > (self.current_frame as u128 * frame_length as u128)
            {
                self.current_frame += 1;

                let mut image_mat_bgra = Mat::default();
                imgproc::cvt_color(
                    &self.display_mat,
                    &mut image_mat_bgra,
                    imgproc::COLOR_BGR2BGRA,
                    4,
                )?;

                // TODO: refactor
                let image_bevy = Image::new(
                    Extent3d {
                        width: stream.plane.w().into(),
                        height: stream.plane.h().into(),
                        depth_or_array_layers: 1,
                    },
                    TextureDimension::D2,
                    Vec::from(image_mat_bgra.data_bytes()?),
                    TextureFormat::Bgra8UnormSrgb,
                );
                break Some(image_bevy);
            }

            match stream.decode_event() {
                Ok(event) if event.d <= D_ZERO_INTEGRATION => {
                    event_count += 1;
                    let y = event.coord.y as i32;
                    let x = event.coord.x as i32;
                    let c = event.coord.c.unwrap_or(0) as i32;
                    if (y | x | c) == 0x0 {
                        self.stream_state.current_t_ticks += event.delta_t;
                    }

                    // TODO: Support D and Dt view modes here

                    let frame_intensity = (event_to_intensity(&event) * stream.ref_interval as f64)
                        / match stream.source_camera {
                            SourceCamera::FramedU8 => u8::MAX as f64,
                            SourceCamera::FramedU16 => u16::MAX as f64,
                            SourceCamera::FramedU32 => u32::MAX as f64,
                            SourceCamera::FramedU64 => u64::MAX as f64,
                            SourceCamera::FramedF32 => {
                                todo!("Not yet implemented")
                            }
                            SourceCamera::FramedF64 => {
                                todo!("Not yet implemented")
                            }
                            SourceCamera::Dvs => u8::MAX as f64,
                            SourceCamera::DavisU8 => u8::MAX as f64,
                            SourceCamera::Atis => {
                                todo!("Not yet implemented")
                            }
                            SourceCamera::Asint => {
                                todo!("Not yet implemented")
                            }
                        }
                        * 255.0;

                    let db = display_mat.data_bytes_mut()?;
                    db[(y as usize * stream.plane.area_wc()
                        + x as usize * stream.plane.c_usize()
                        + c as usize)] = frame_intensity as u8;
                }
                Err(_e) => {
                    match stream.set_input_stream_position(stream.header_size as u64) {
                        Ok(_) => {}
                        Err(ee) => {
                            eprintln!("{ee}")
                        }
                    };
                    self.frame_sequence =
                        self.framer_builder.clone().map(|builder| builder.finish());
                    // if !self.ui_state.looping {
                    //     self.ui_state.playing = false;
                    // }
                    self.stream_state.current_t_ticks = 0;

                    break None;
                }
                _ => {}
            }
        };

        Ok((event_count, image_bevy))
    }

    fn consume_source_accurate(&mut self) -> Result<PlayerArtifact, Box<dyn Error>> {
        let mut event_count = 0;

        let stream = match &mut self.input_stream {
            None => {
                return Ok((event_count, None));
            }
            Some(s) => s,
        };

        let frame_sequence = match &mut self.frame_sequence {
            None => {
                return Ok((event_count, None));
            }
            Some(s) => s,
        };

        let display_mat = &mut self.display_mat;

        let image_bevy = if frame_sequence.is_frame_0_filled() {
            let mut idx = 0;
            for chunk_num in 0..frame_sequence.get_frame_chunks_num() {
                match frame_sequence.pop_next_frame_for_chunk(chunk_num) {
                    Some(arr) => {
                        for px in arr.iter() {
                            match px {
                                Some(event) => {
                                    let db = display_mat.data_bytes_mut()?;
                                    db[idx] = *event;
                                    idx += 1;
                                }
                                None => {}
                            };
                        }
                    }
                    None => {
                        println!("Couldn't pop chunk {chunk_num}!")
                    }
                }
            }
            frame_sequence.state.frames_written += 1;
            self.stream_state.current_t_ticks += frame_sequence.state.tpf;

            let mut image_mat_bgra = Mat::default();
            imgproc::cvt_color(display_mat, &mut image_mat_bgra, imgproc::COLOR_BGR2BGRA, 4)?;

            // TODO: refactor
            Some(Image::new(
                Extent3d {
                    width: stream.plane.w().into(),
                    height: stream.plane.h().into(),
                    depth_or_array_layers: 1,
                },
                TextureDimension::D2,
                Vec::from(image_mat_bgra.data_bytes()?),
                TextureFormat::Bgra8UnormSrgb,
            ))
        } else {
            None
        };

        loop {
            match stream.decode_event() {
                Ok(mut event) => {
                    event_count += 1;
                    if frame_sequence.ingest_event(&mut event) {
                        return Ok((event_count, image_bevy));
                    }
                }
                Err(_e) => {
                    // Loop back to the beginning
                    stream.set_input_stream_position(stream.header_size as u64)?;

                    self.frame_sequence =
                        self.framer_builder.clone().map(|builder| builder.finish());
                    return Ok((event_count, image_bevy));
                }
            }
        }
    }
}
