use crate::player::ui::ReconstructionMethod;
use adder_codec_core::bitstream_io::{BigEndian, BitReader};
use adder_codec_core::codec::decoder::Decoder;
use adder_codec_core::*;
use adder_codec_rs::framer::driver::FramerMode::INSTANTANEOUS;
use adder_codec_rs::framer::driver::{FrameSequence, Framer, FramerBuilder};
use adder_codec_rs::framer::scale_intensity::event_to_intensity;

#[cfg(feature = "open-cv")]
use adder_codec_rs::transcoder::source::video::show_display_force;
use adder_codec_rs::transcoder::source::video::FramedViewMode;
use bevy::prelude::Image;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use ndarray::Array;
use ndarray::Array3;
#[cfg(feature = "open-cv")]
use opencv::core::{
    create_continuous, KeyPoint, Mat, MatTraitConstManual, MatTraitManual, Scalar, Vector, CV_8UC1,
    CV_8UC3,
};
#[cfg(feature = "open-cv")]
use opencv::imgproc;
use std::error::Error;
use std::fmt;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

pub type PlayerArtifact = (u64, Option<Image>);
pub type PlayerStreamArtifact = (u64, StreamState, Option<Image>);

#[derive(Default, Clone, Debug)]
pub struct StreamState {
    pub(crate) current_t_ticks: DeltaT,
    pub(crate) tps: DeltaT,
    pub(crate) file_pos: u64,
    pub(crate) volume: usize,
    pub(crate) last_timestamps: Array3<DeltaT>,
    // The current instantaneous frame, for determining features
    pub running_intensities: Array3<i32>,
}

// TODO: allow flexibility with decoding non-file inputs
pub struct InputStream {
    pub(crate) decoder: Decoder<BufReader<File>>,
    pub(crate) bitreader: BitReader<BufReader<File>, BigEndian>,
}
unsafe impl Send for InputStream {}

#[derive(Default)]
pub struct AdderPlayer {
    pub(crate) framer_builder: Option<FramerBuilder>,
    pub(crate) frame_sequence: Option<FrameSequence<u8>>, // TODO: remove this
    pub(crate) input_stream: Option<InputStream>,
    pub(crate) display_mat: Mat,
    playback_speed: f32,
    reconstruction_method: ReconstructionMethod,
    current_frame: u32,
    stream_state: StreamState,
    pub(crate) view_mode: FramedViewMode,
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
        detect_features: bool,
    ) -> Result<Self, Box<dyn Error>> {
        match path_buf.extension() {
            None => Err(Box::new(AdderPlayerError("Invalid file type".into()))),
            Some(ext) => match ext.to_ascii_lowercase().to_str() {
                None => Err(Box::new(AdderPlayerError("Invalid file type".into()))),
                Some("adder") => {
                    let input_path = path_buf.to_str().expect("Invalid string").to_string();
                    let (stream, bitreader) = open_file_decoder(&input_path)?;

                    let meta = *stream.meta();
                    let mut reconstructed_frame_rate = (meta.tps / meta.ref_interval) as f32;

                    reconstructed_frame_rate /= playback_speed as f32;

                    let framer_builder: FramerBuilder = FramerBuilder::new(meta.plane, 260)
                        .codec_version(meta.codec_version, meta.time_mode)
                        .time_parameters(
                            meta.tps,
                            meta.ref_interval,
                            meta.delta_t_max,
                            reconstructed_frame_rate,
                        )
                        .mode(INSTANTANEOUS)
                        .view_mode(view_mode)
                        .detect_features(detect_features)
                        .source(stream.get_source_type(), meta.source_camera);

                    let frame_sequence: FrameSequence<u8> = framer_builder.clone().finish();

                    let mut display_mat = Mat::default();
                    match meta.plane.c() {
                        1 => {
                            create_continuous(
                                meta.plane.h() as i32,
                                meta.plane.w() as i32,
                                CV_8UC1,
                                &mut display_mat,
                            )?;
                        }
                        3 => {
                            create_continuous(
                                meta.plane.h() as i32,
                                meta.plane.w() as i32,
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
                            tps: meta.tps,
                            file_pos: 0,
                            volume: meta.plane.volume(),
                            running_intensities: Array::zeros((
                                meta.plane.h_usize(),
                                meta.plane.w_usize(),
                                meta.plane.c_usize(),
                            )),
                            last_timestamps: Array::zeros((
                                meta.plane.h_usize(),
                                meta.plane.w_usize(),
                                meta.plane.c_usize(),
                            )),
                        },
                        framer_builder: Some(framer_builder),
                        frame_sequence: Some(frame_sequence),
                        input_stream: Some(InputStream {
                            decoder: stream,
                            bitreader,
                        }),
                        display_mat,
                        playback_speed,
                        reconstruction_method: ReconstructionMethod::Accurate,
                        current_frame: 0,
                        view_mode,
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
            if pos > stream.decoder.meta().header_size as u64 {
                match stream
                    .decoder
                    .set_input_stream_position(&mut stream.bitreader, pos)
                {
                    Ok(_) => {}
                    Err(_) => {}
                }
            } else {
                match stream.decoder.set_input_stream_position(
                    &mut stream.bitreader,
                    stream.decoder.meta().header_size as u64,
                ) {
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
                return (0, self.stream_state.clone(), None);
            }
            Some(s) => s,
        };

        // Reset the stats if we're starting a new looped playback of the video
        if let Ok(pos) = stream
            .decoder
            .get_input_stream_position(&mut stream.bitreader)
        {
            if pos == stream.decoder.meta().header_size as u64 {
                match &mut self.frame_sequence {
                    None => {
                        // TODO: error
                        eprintln!("TODO Error");
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
            Some(s) => s
                .decoder
                .get_input_stream_position(&mut s.bitreader)
                .unwrap_or(0),
        };
        match res {
            Ok(a) => (a.0, self.stream_state.clone(), a.1),
            Err(_b) => (0, self.stream_state.clone(), None),
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

        let meta = *stream.decoder.meta();

        let frame_length = meta.ref_interval as f64 * self.playback_speed as f64; //TODO: temp

        let mut display_mat = &mut self.display_mat;

        #[cfg(feature = "open-cv")]
        if self.view_mode == FramedViewMode::DeltaT {
            opencv::core::normalize(
                &display_mat.clone(),
                &mut display_mat,
                0.0,
                255.0,
                opencv::core::NORM_MINMAX,
                opencv::core::CV_8U,
                &Mat::default(),
            )?;
            opencv::core::subtract(
                &Scalar::new(255.0, 255.0, 255.0, 0.0),
                &display_mat.clone(),
                &mut display_mat,
                &Mat::default(),
                opencv::core::CV_8U,
            )?;
        }

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
                        width: meta.plane.w().into(),
                        height: meta.plane.h().into(),
                        depth_or_array_layers: 1,
                    },
                    TextureDimension::D2,
                    Vec::from(image_mat_bgra.data_bytes()?),
                    TextureFormat::Bgra8UnormSrgb,
                );
                break Some(image_bevy);
            }

            match stream.decoder.digest_event(&mut stream.bitreader) {
                Ok(mut event) if event.d <= D_ZERO_INTEGRATION => {
                    event_count += 1;
                    let y = event.coord.y as i32;
                    let x = event.coord.x as i32;
                    let c = event.coord.c.unwrap_or(0) as i32;
                    // if (y | x | c) == 0x0 {
                    //     self.stream_state.current_t_ticks += event.delta_t;
                    // }

                    if meta.time_mode == TimeMode::AbsoluteT {
                        if event.delta_t > self.stream_state.current_t_ticks {
                            self.stream_state.current_t_ticks = event.delta_t;
                        }
                        let dt = event.delta_t
                            - self.stream_state.last_timestamps
                                [[y as usize, x as usize, c as usize]];
                        self.stream_state.last_timestamps[[y as usize, x as usize, c as usize]] =
                            event.delta_t;
                        if self.stream_state.last_timestamps[[y as usize, x as usize, c as usize]]
                            % meta.ref_interval
                            != 0
                        {
                            self.stream_state.last_timestamps
                                [[y as usize, x as usize, c as usize]] = ((self
                                .stream_state
                                .last_timestamps[[y as usize, x as usize, c as usize]]
                                / meta.ref_interval)
                                + 1)
                                * meta.ref_interval;
                        }
                        event.delta_t = dt;
                    } else {
                        self.stream_state.last_timestamps[[y as usize, x as usize, c as usize]] +=
                            event.delta_t;
                        if self.stream_state.last_timestamps[[y as usize, x as usize, c as usize]]
                            % meta.ref_interval
                            != 0
                        {
                            self.stream_state.last_timestamps
                                [[y as usize, x as usize, c as usize]] = ((self
                                .stream_state
                                .last_timestamps[[y as usize, x as usize, c as usize]]
                                / meta.ref_interval)
                                + 1)
                                * meta.ref_interval;
                        }

                        if self.stream_state.last_timestamps[[y as usize, x as usize, c as usize]]
                            > self.stream_state.current_t_ticks
                        {
                            self.stream_state.current_t_ticks = self.stream_state.last_timestamps
                                [[y as usize, x as usize, c as usize]];
                        }
                    }

                    // TODO: Support D and Dt view modes here

                    let frame_intensity = (event_to_intensity(&event) * meta.ref_interval as f64)
                        / match meta.source_camera {
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
                    db[y as usize * meta.plane.area_wc()
                        + x as usize * meta.plane.c_usize()
                        + c as usize] = frame_intensity as u8;
                }
                Err(_e) => {
                    match stream
                        .decoder
                        .set_input_stream_position(&mut stream.bitreader, meta.header_size as u64)
                    {
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
                _ => {
                    eprintln!("???");
                }
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
        let meta = *stream.decoder.meta();

        let frame_sequence = match &mut self.frame_sequence {
            None => {
                return Ok((event_count, None));
            }
            Some(s) => s,
        };

        let mut display_mat = &mut self.display_mat;

        let image_bevy = if frame_sequence.is_frame_0_filled() {
            let mut idx = 0;
            let db = display_mat.data_bytes_mut()?;
            for chunk_num in 0..frame_sequence.get_frame_chunks_num() {
                match frame_sequence.pop_next_frame_for_chunk(chunk_num) {
                    Some(arr) => {
                        for px in arr.iter() {
                            match px {
                                Some(event) => {
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

            // TODO: temporary, for testing what the running intensities look like
            // let running_intensities = frame_sequence.get_running_intensities();
            // for px in running_intensities.iter() {
            //     db[idx] = *px as u8;
            //     idx += 1;
            // }

            if let Some(features) = frame_sequence.pop_features() {
                for feature in features {
                    let db = display_mat.data_bytes_mut()?;

                    let color: u8 = 255;
                    let radius = 2;
                    for i in -radius..=radius {
                        let idx = ((feature.y as i32 + i) * meta.plane.w() as i32
                            + feature.x as i32) as usize;
                        db[idx] = color;

                        let idx = (feature.y as i32 * meta.plane.w() as i32
                            + (feature.x as i32 + i)) as usize;
                        db[idx] = color;
                    }
                }
            }

            #[cfg(feature = "open-cv")]
            if self.view_mode == FramedViewMode::DeltaT {
                opencv::core::normalize(
                    &display_mat.clone(),
                    &mut display_mat,
                    0.0,
                    255.0,
                    opencv::core::NORM_MINMAX,
                    opencv::core::CV_8U,
                    &Mat::default(),
                )?;
                opencv::core::subtract(
                    &Scalar::new(255.0, 255.0, 255.0, 0.0),
                    &display_mat.clone(),
                    &mut display_mat,
                    &Mat::default(),
                    opencv::core::CV_8U,
                )?;
            } else if self.view_mode == FramedViewMode::D {
            }

            // let mut keypoints = Vector::<KeyPoint>::new();
            // opencv::features2d::fast(display_mat, &mut keypoints, 50, true)?;
            // let mut keypoint_mat = Mat::default();
            // opencv::features2d::draw_keypoints(
            //     display_mat,
            //     &keypoints,
            //     &mut keypoint_mat,
            //     Scalar::new(0.0, 0.0, 255.0, 0.0),
            //     opencv::features2d::DrawMatchesFlags::DEFAULT,
            // )?;
            // show_display_force("keypoints", &keypoint_mat, 1)?;

            frame_sequence.state.frames_written += 1;
            self.stream_state.current_t_ticks += frame_sequence.state.tpf;

            let mut image_mat_bgra = Mat::default();
            imgproc::cvt_color(display_mat, &mut image_mat_bgra, imgproc::COLOR_BGR2BGRA, 4)?;

            // TODO: refactor
            Some(Image::new(
                Extent3d {
                    width: meta.plane.w().into(),
                    height: meta.plane.h().into(),
                    depth_or_array_layers: 1,
                },
                TextureDimension::D2,
                Vec::from(image_mat_bgra.data_bytes()?),
                TextureFormat::Bgra8UnormSrgb,
            ))
        } else {
            None
        };

        if image_bevy.is_some() {
            return Ok((0, image_bevy));
        }

        let mut last_event: Option<Event> = None;
        loop {
            match stream.decoder.digest_event(&mut stream.bitreader) {
                Ok(mut event) => {
                    event_count += 1;
                    let filled = frame_sequence.ingest_event(&mut event, last_event);

                    last_event = Some(event.clone());

                    if filled {
                        return Ok((event_count, image_bevy));
                    }
                }
                Err(e) => {
                    eprintln!("{}", e);

                    // TODO: Need to reset the UI event count events_ppc count when looping back here
                    // Loop/restart back to the beginning
                    stream.decoder.set_input_stream_position(
                        &mut stream.bitreader,
                        meta.header_size as u64,
                    )?;

                    self.frame_sequence =
                        self.framer_builder.clone().map(|builder| builder.finish());
                    return Ok((event_count, image_bevy));
                }
            }
        }
    }
}
