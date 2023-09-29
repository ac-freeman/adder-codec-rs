use crate::player::ui::ReconstructionMethod;
use adder_codec_core::bitstream_io::{BigEndian, BitReader};
use adder_codec_core::codec::decoder::Decoder;
use adder_codec_core::*;
use adder_codec_rs::framer::driver::FramerMode::INSTANTANEOUS;
use adder_codec_rs::framer::driver::{FrameSequence, Framer, FramerBuilder};
use adder_codec_rs::framer::scale_intensity::event_to_intensity;
use adder_codec_rs::transcoder::source::video::{show_display_force, FramedViewMode};
use bevy::prelude::Image;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use ndarray::Array3;
use opencv::core::{
    create_continuous, KeyPoint, Mat, MatTraitConstManual, MatTraitManual, Scalar, Vector, CV_8UC1,
    CV_8UC3,
};
use opencv::imgproc;
use std::error::Error;
use std::fmt;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

pub type PlayerArtifact = (u64, Option<Image>);
pub type PlayerStreamArtifact = (u64, StreamState, Option<Image>);

#[derive(Default, Copy, Clone, Debug)]
pub struct StreamState {
    pub(crate) current_t_ticks: DeltaT,
    pub(crate) tps: DeltaT,
    pub(crate) file_pos: u64,
    pub(crate) volume: usize,
    // The current instantaneous frame, for determining features
    // pub running_intensities: Array3<i32>,
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
    ) -> Result<Self, Box<dyn Error>> {
        match path_buf.extension() {
            None => Err(Box::new(AdderPlayerError("Invalid file type".into()))),
            Some(ext) => match ext.to_ascii_lowercase().to_str() {
                None => Err(Box::new(AdderPlayerError("Invalid file type".into()))),
                Some("adder") => {
                    let input_path = path_buf.to_str().expect("Invalid string").to_string();
                    let (stream, bitreader) = open_file_decoder(&input_path)?;

                    let meta = *stream.meta();
                    let mut reconstructed_frame_rate = (meta.tps / meta.ref_interval) as f64;

                    reconstructed_frame_rate /= playback_speed as f64;

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
                return (0, self.stream_state, None);
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

        let meta = *stream.decoder.meta();

        let frame_length = meta.ref_interval as f64 * self.playback_speed as f64; //TODO: temp

        let mut display_mat = &mut self.display_mat;

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
                Ok(event) if event.d <= D_ZERO_INTEGRATION => {
                    event_count += 1;
                    let y = event.coord.y as i32;
                    let x = event.coord.x as i32;
                    let c = event.coord.c.unwrap_or(0) as i32;
                    if (y | x | c) == 0x0 {
                        self.stream_state.current_t_ticks += event.delta_t;
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
                // Loop through each element and find all the ones that have neighboring pixels
                // in two directions that have a different D value. If so, set the pixel to white.

                // let mut corner_mat = Mat::new_rows_cols_with_default(
                //     meta.plane.h() as i32,
                //     meta.plane.w() as i32,
                //     opencv::core::CV_8U,
                //     Scalar::new(0.0, 0.0, 0.0, 0.0),
                // )?
                // .clone();
                //
                // let db = display_mat.data_bytes()?;
                // let corner_db = corner_mat.data_bytes_mut()?;
                // // Loop through the pixels
                // for y in 0..meta.plane.h() {
                //     for x in 0..meta.plane.w() {
                //         let idx = y as usize * meta.plane.w_usize() + x as usize;
                //
                //         let mut neighbors = vec![255; 4];
                //         let mut neighbors_2 = vec![255; 4];
                //         let mut neighbors_3 = vec![255; 4];
                //         let mut neighbors_4 = vec![255; 4];
                //
                //         // Left
                //         if x > 3 {
                //             neighbors[0] = db[idx - 1];
                //             neighbors_2[0] = db[idx - 2];
                //             neighbors_3[0] = db[idx - 3];
                //             neighbors_4[0] = db[idx - 4];
                //         }
                //         // Up
                //         if y > 3 {
                //             neighbors[1] = db[idx - meta.plane.w_usize()];
                //             neighbors_2[1] = db[idx - meta.plane.w_usize() * 2];
                //             neighbors_3[1] = db[idx - meta.plane.w_usize() * 3];
                //             neighbors_4[1] = db[idx - meta.plane.w_usize() * 4];
                //         }
                //         // Right
                //         if x < meta.plane.w() - 4 {
                //             neighbors[2] = db[idx + 1];
                //             neighbors_2[2] = db[idx + 2];
                //             neighbors_3[2] = db[idx + 3];
                //             neighbors_4[2] = db[idx + 4];
                //         }
                //
                //         // Down
                //         if y < meta.plane.h() - 4 {
                //             neighbors[3] = db[idx + meta.plane.w_usize()];
                //             neighbors_2[3] = db[idx + meta.plane.w_usize() * 2];
                //             neighbors_3[3] = db[idx + meta.plane.w_usize() * 3];
                //             neighbors_4[3] = db[idx + meta.plane.w_usize() * 4];
                //         }
                //
                //         // Check
                //         let mut count = 0;
                //         let mut window_num = 0;
                //         neighbors.windows(2).enumerate().for_each(|(index, w)| {
                //             if w[0] == db[idx] && w[1] == db[idx] {
                //                 // corner_db[idx] = 255;
                //                 count += 1;
                //                 window_num = index;
                //             }
                //         });
                //         if neighbors[0] == db[idx] && neighbors[3] == db[idx] {
                //             // corner_db[idx] = 255;
                //             count += 1;
                //             window_num = 3;
                //         }
                //
                //         if count == 1 {
                //             // corner_db[idx] = 255;
                //             // Check neighbors_2
                //             match window_num {
                //                 0 => {
                //                     if neighbors_2[0] == db[idx]
                //                         && neighbors_2[1] == db[idx]
                //                         && neighbors_3[0] == db[idx]
                //                         && neighbors_3[1] == db[idx]
                //                         && neighbors_4[0] == db[idx]
                //                         && neighbors_4[1] == db[idx]
                //                     {
                //                         corner_db[idx] = 255;
                //                     }
                //                 }
                //                 1 => {
                //                     if neighbors_2[1] == db[idx]
                //                         && neighbors_2[2] == db[idx]
                //                         && neighbors_3[1] == db[idx]
                //                         && neighbors_3[2] == db[idx]
                //                         && neighbors_4[1] == db[idx]
                //                         && neighbors_4[2] == db[idx]
                //                     {
                //                         corner_db[idx] = 255;
                //                     }
                //                 }
                //                 2 => {
                //                     if neighbors_2[2] == db[idx]
                //                         && neighbors_2[3] == db[idx]
                //                         && neighbors_3[2] == db[idx]
                //                         && neighbors_3[3] == db[idx]
                //                         && neighbors_4[2] == db[idx]
                //                         && neighbors_4[3] == db[idx]
                //                     {
                //                         corner_db[idx] = 255;
                //                     }
                //                 }
                //                 3 => {
                //                     if neighbors_2[3] == db[idx]
                //                         && neighbors_2[0] == db[idx]
                //                         && neighbors_3[3] == db[idx]
                //                         && neighbors_3[0] == db[idx]
                //                         && neighbors_4[3] == db[idx]
                //                         && neighbors_4[0] == db[idx]
                //                     {
                //                         corner_db[idx] = 255;
                //                     }
                //                 }
                //                 _ => {}
                //             }
                //         }
                //
                //         // if neighbors.iter().filter(|&x| *x != db[idx]).count() == 2 {
                //         //     corner_db[idx] = 255;
                //         // } else {
                //         //     corner_db[idx] = 0;
                //         // }
                //     }
                // }
                // show_display_force("cornerss", &corner_mat, 1)?;
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
                    let filled = frame_sequence.ingest_event(&mut event);

                    if let Some(last) = last_event {
                        if event.delta_t != last.delta_t {
                            // TODO: Do the feature test
                        }
                    }

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
