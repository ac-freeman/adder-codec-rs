use crate::player::ui::ReconstructionMethod;
use adder_codec_rs::adder_codec_core::bitstream_io::{BigEndian, BitReader};
use adder_codec_rs::adder_codec_core::codec::decoder::Decoder;
use adder_codec_rs::adder_codec_core::*;
use adder_codec_rs::framer::driver::FramerMode::INSTANTANEOUS;
use adder_codec_rs::framer::driver::{FrameSequence, Framer, FramerBuilder};
use adder_codec_rs::framer::scale_intensity::event_to_intensity;

use crate::utils::prep_bevy_image;
#[cfg(feature = "open-cv")]
use adder_codec_rs::transcoder::source::video::show_display_force;
use adder_codec_rs::transcoder::source::video::FramedViewMode;
use adder_codec_rs::utils::cv::is_feature;
use adder_codec_rs::utils::viz::draw_feature_coord;
use bevy::prelude::Image;
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
use video_rs_adder_dep::Frame;

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
    pub(crate) display_frame: Frame,
    pub(crate) running_intensities: Array3<u8>,
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
        buffer_limit: Option<u32>,
    ) -> Result<Self, Box<dyn Error>> {
        match path_buf.extension() {
            None => Err(Box::new(AdderPlayerError("Invalid file type".into()))),
            Some(ext) => match ext.to_ascii_lowercase().to_str() {
                None => Err(Box::new(AdderPlayerError("Invalid file type".into()))),
                Some("adder") => {
                    let input_path = path_buf.to_str().expect("Invalid string").to_string();
                    let (stream, bitreader) = open_file_decoder(&input_path)?;

                    let meta = *stream.meta();

                    let mut reconstructed_frame_rate = meta.tps as f32 / meta.ref_interval as f32;

                    reconstructed_frame_rate /= playback_speed as f32;

                    let framer_builder: FramerBuilder = FramerBuilder::new(meta.plane, 260)
                        .codec_version(meta.codec_version, meta.time_mode)
                        .time_parameters(
                            meta.tps,
                            meta.ref_interval,
                            meta.delta_t_max,
                            Some(reconstructed_frame_rate),
                        )
                        .mode(INSTANTANEOUS)
                        .buffer_limit(buffer_limit)
                        .view_mode(view_mode)
                        .detect_features(detect_features)
                        .source(stream.get_source_type(), meta.source_camera);

                    let frame_sequence: FrameSequence<u8> = framer_builder.clone().finish();

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
                        display_frame: Array3::zeros((
                            meta.plane.h_usize(),
                            meta.plane.w_usize(),
                            meta.plane.c_usize(),
                        )),
                        running_intensities: Array3::zeros((
                            meta.plane.h_usize(),
                            meta.plane.w_usize(),
                            1,
                        )),
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

    pub fn consume_source(&mut self, detect_features: bool) -> PlayerStreamArtifact {
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
                        frame_sequence.state.reset();
                    }
                };
            }
        }

        let res = match self.reconstruction_method {
            ReconstructionMethod::Fast => self.consume_source_fast(detect_features),
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

    fn consume_source_fast(
        &mut self,
        detect_features: bool,
    ) -> Result<PlayerArtifact, Box<dyn Error>> {
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

        // if self.view_mode == FramedViewMode::DeltaT {
        //     opencv::core::normalize(
        //         &display_mat.clone(),
        //         &mut display_mat,
        //         0.0,
        //         255.0,
        //         opencv::core::NORM_MINMAX,
        //         opencv::core::CV_8U,
        //         &Mat::default(),
        //     )?;
        //     opencv::core::subtract(
        //         &Scalar::new(255.0, 255.0, 255.0, 0.0),
        //         &display_mat.clone(),
        //         &mut display_mat,
        //         &Mat::default(),
        //         opencv::core::CV_8U,
        //     )?;
        // }

        let image_bevy = loop {
            let mut display_mat = &mut self.display_frame;
            let color = display_mat.shape()[2] == 3;

            if self.stream_state.current_t_ticks as u128
                > (self.current_frame as u128 * frame_length as u128)
            {
                self.current_frame += 1;

                let image_bevy =
                    prep_bevy_image(display_mat.clone(), color, meta.plane.w(), meta.plane.h())?;
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
                        if event.t > self.stream_state.current_t_ticks {
                            self.stream_state.current_t_ticks = event.t;
                        }

                        let dt = event.t
                            - self.stream_state.last_timestamps
                                [[y as usize, x as usize, c as usize]];
                        self.stream_state.last_timestamps[[y as usize, x as usize, c as usize]] =
                            event.t;
                        if is_framed(meta.source_camera)
                            && self.stream_state.last_timestamps
                                [[y as usize, x as usize, c as usize]]
                                % meta.ref_interval
                                != 0
                        {
                            // If it's a framed source, make the timestamp align to the reference interval

                            self.stream_state.last_timestamps
                                [[y as usize, x as usize, c as usize]] = ((self
                                .stream_state
                                .last_timestamps[[y as usize, x as usize, c as usize]]
                                / meta.ref_interval)
                                + 1)
                                * meta.ref_interval;
                        }
                        event.t = dt;
                    } else {
                        panic!("Relative time mode is deprecated.");
                        self.stream_state.last_timestamps[[y as usize, x as usize, c as usize]] +=
                            event.t;
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

                    unsafe {
                        *display_mat.uget_mut((
                            event.coord.y as usize,
                            event.coord.x as usize,
                            event.coord.c.unwrap_or(0) as usize,
                        )) = frame_intensity as u8;

                        if detect_features && event.coord.c.unwrap_or(0) == 0 {
                            *self.running_intensities.uget_mut((
                                event.coord.y as usize,
                                event.coord.x as usize,
                                0,
                            )) = frame_intensity as u8;

                            // Test if this is a feature
                            if is_feature(event.coord, meta.plane, &self.running_intensities)? {
                                draw_feature_coord(
                                    event.coord.x,
                                    event.coord.y,
                                    &mut display_mat,
                                    color,
                                );
                            } else if !event.coord.is_border(
                                meta.plane.w_usize(),
                                meta.plane.h_usize(),
                                3,
                            ) {
                                // Reset the pixels in the cross accordingly...
                                let radius = 2;
                                if color {
                                    for i in -radius..=radius {
                                        for c in 0..3 as usize {
                                            *display_mat.uget_mut((
                                                (event.coord.y as i32 + i) as usize,
                                                (event.coord.x as i32) as usize,
                                                c,
                                            )) = *self.running_intensities.uget((
                                                (event.coord.y as i32 + i) as usize,
                                                (event.coord.x as i32) as usize,
                                                c,
                                            ));
                                            *display_mat.uget_mut((
                                                (event.coord.y as i32) as usize,
                                                (event.coord.x as i32 + i) as usize,
                                                c,
                                            )) = *self.running_intensities.uget((
                                                (event.coord.y as i32) as usize,
                                                (event.coord.x as i32 + i) as usize,
                                                c,
                                            ));
                                        }
                                    }
                                } else {
                                    for i in -radius..=radius {
                                        *display_mat.uget_mut((
                                            (event.coord.y as i32 + i) as usize,
                                            (event.coord.x as i32) as usize,
                                            0,
                                        )) = *self.running_intensities.uget((
                                            (event.coord.y as i32 + i) as usize,
                                            (event.coord.x as i32) as usize,
                                            0,
                                        ));
                                        *display_mat.uget_mut((
                                            (event.coord.y as i32) as usize,
                                            (event.coord.x as i32 + i) as usize,
                                            0,
                                        )) = *self.running_intensities.uget((
                                            (event.coord.y as i32) as usize,
                                            (event.coord.x as i32 + i) as usize,
                                            0,
                                        ));
                                    }
                                }
                            }
                        }
                    }
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
                    self.stream_state.last_timestamps = Array::zeros((
                        meta.plane.h_usize(),
                        meta.plane.w_usize(),
                        meta.plane.c_usize(),
                    ));
                    self.stream_state.current_t_ticks = 0;
                    self.current_frame = 0;

                    break None;
                }
                _ => {
                    // Got an event with 0 integration, so don't need to update a pixel value
                    // eprintln!("???");
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

        let display_mat = &mut self.display_frame;

        let image_bevy = if frame_sequence.is_frame_0_filled() {
            let mut idx = 0;
            let db = display_mat.as_slice_mut().unwrap();
            let new_frame = frame_sequence.pop_next_frame().unwrap();
            for chunk in new_frame {
                // match frame_sequence.pop_next_frame_for_chunk(chunk_num) {
                //     Some(arr) => {
                for px in chunk.iter() {
                    match px {
                        Some(event) => {
                            db[idx] = *event;
                        }
                        None => {}
                    };
                    idx += 1;
                }
                // }
                // None => {
                //     println!("Couldn't pop chunk {chunk_num}!")
                // }
                // }
            }

            // TODO: temporary, for testing what the running intensities look like
            // let running_intensities = frame_sequence.get_running_intensities();
            // for px in running_intensities.iter() {
            //     db[idx] = *px as u8;
            //     idx += 1;
            // }

            if let Some(feature_interval) = frame_sequence.pop_features() {
                for feature in feature_interval.features {
                    let db = display_mat.as_slice_mut().unwrap();

                    let color: u8 = 255;
                    let radius = 2;
                    for i in -radius..=radius {
                        let idx =
                            ((feature.y as i32 + i) * meta.plane.w() as i32 * meta.plane.c() as i32
                                + (feature.x as i32) * meta.plane.c() as i32)
                                as usize;
                        db[idx] = color;

                        if meta.plane.c() > 1 {
                            db[idx + 1] = color;
                            db[idx + 2] = color;
                        }

                        let idx = (feature.y as i32 * meta.plane.w() as i32 * meta.plane.c() as i32
                            + (feature.x as i32 + i) * meta.plane.c() as i32)
                            as usize;
                        db[idx] = color;

                        if meta.plane.c() > 1 {
                            db[idx + 1] = color;
                            db[idx + 2] = color;
                        }
                    }
                }
            }

            // if self.view_mode == FramedViewMode::DeltaT {
            //     opencv::core::normalize(
            //         &display_mat.clone(),
            //         &mut display_mat,
            //         0.0,
            //         255.0,
            //         opencv::core::NORM_MINMAX,
            //         opencv::core::CV_8U,
            //         &Mat::default(),
            //     )?;
            //     opencv::core::subtract(
            //         &Scalar::new(255.0, 255.0, 255.0, 0.0),
            //         &display_mat.clone(),
            //         &mut display_mat,
            //         &Mat::default(),
            //         opencv::core::CV_8U,
            //     )?;
            // } else if self.view_mode == FramedViewMode::D {
            // }

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

            self.stream_state.current_t_ticks += frame_sequence.state.tpf;

            let image_mat = self.display_frame.clone();
            let color = image_mat.shape()[2] == 3;

            let image_bevy = prep_bevy_image(image_mat, color, meta.plane.w(), meta.plane.h())?;

            Some(image_bevy)
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
                    eprintln!("Player error: {}", e);
                    if !frame_sequence.flush_frame_buffer() {
                        eprintln!("Completely done");
                        // TODO: Need to reset the UI event count events_ppc count when looping back here
                        // Loop/restart back to the beginning
                        stream.decoder.set_input_stream_position(
                            &mut stream.bitreader,
                            meta.header_size as u64,
                        )?;

                        self.frame_sequence =
                            self.framer_builder.clone().map(|builder| builder.finish());
                        return Ok((event_count, image_bevy));
                    } else {
                        return self.consume_source_accurate();
                    }
                }
            }
        }
    }
}
