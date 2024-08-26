use crate::player::adder::AdderPlayerError::Uninitialized;
use crate::player::adder::AdderPlayerError::{InvalidFileType, NoFileSelected};
use crate::player::ui::PlayerState;
use crate::player::ui::{PlayerInfoMsg, PlayerStateMsg};
use crate::utils::prep_epaint_image;
use adder_codec_rs::adder_codec_core::bitstream_io::{BigEndian, BitReader};
use adder_codec_rs::adder_codec_core::codec::decoder::Decoder;
use adder_codec_rs::adder_codec_core::codec::{CodecError, EncoderType};
use adder_codec_rs::adder_codec_core::{is_framed, open_file_decoder, Event};
use adder_codec_rs::framer::driver::FramerMode::INSTANTANEOUS;
use adder_codec_rs::framer::driver::{FrameSequence, Framer, FramerBuilder};
use eframe::epaint::ColorImage;
use ndarray::Array3;
use std::fs::File;
use std::io::BufReader;
use std::time::{Duration, Instant};
use thiserror::Error;
use tokio::sync::mpsc;
use tokio::sync::mpsc::error::TrySendError;
use tokio::sync::mpsc::{Receiver, Sender};
use video_rs_adder_dep::Frame;

#[derive(Error, Debug)]
pub enum AdderPlayerError {
    /// Input file error
    #[error("Invalid file type")]
    InvalidFileType,

    /// No file selected
    #[error("No file selected")]
    NoFileSelected,

    /// Uninitialized error
    #[error("Uninitialized")]
    Uninitialized,

    /// Codec error
    #[error("Codec error")]
    CodecError(#[from] CodecError),
    // /// Everything else error
    // #[error(transparent)]
    // Other(#[from] anyhow::Error),
}

pub struct AdderPlayer {
    pool: tokio::runtime::Runtime,
    player_state: PlayerState,
    framer: Option<FrameSequence<u8>>,
    // source: Option<dyn Framer<Output=()>>,
    rx: Receiver<PlayerStateMsg>,
    msg_tx: mpsc::Sender<PlayerInfoMsg>,
    // pub(crate) adder_image_handle: egui::TextureHandle,
    // adder_image_tx: Sender<ColorImage>,
    total_events: u64,
    last_consume_time: std::time::Instant,
    input_stream: Option<InputStream>,
    running_frame: Frame,
    pub image_tx: Sender<ColorImage>,
}

impl AdderPlayer {
    pub(crate) fn new(
        rx: Receiver<PlayerStateMsg>,
        msg_tx: mpsc::Sender<PlayerInfoMsg>,
        image_tx: Sender<ColorImage>,
    ) -> Self {
        let threaded_rt = tokio::runtime::Runtime::new().unwrap();

        AdderPlayer {
            pool: threaded_rt,
            player_state: Default::default(),
            image_tx,
            total_events: 0,
            last_consume_time: std::time::Instant::now(),
            framer: None,
            rx,
            msg_tx,
            input_stream: None,
            running_frame: Frame::zeros((0, 0, 0)),
        }
    }

    pub(crate) async fn run(&mut self) {
        loop {
            match self.rx.try_recv() {
                Ok(msg) => match msg {
                    PlayerStateMsg::Terminate => {
                        eprintln!("Resetting video");
                        todo!();
                    }
                    PlayerStateMsg::Loop { player_state } => {
                        eprintln!("Looping video");
                        let result = self.state_update(player_state, true);
                        self.handle_error(result);
                    }
                    PlayerStateMsg::Set { player_state } => {
                        eprintln!("Received player state");
                        let result = self.state_update(player_state, false);
                        self.handle_error(result);
                    }
                },
                Err(_) => {
                    // Received no data, so consume the transcoder source if it exists
                    if self.framer.is_some() {
                        let result = self.consume().await;
                        self.handle_error(result);
                    }
                }
            }
        }
    }

    fn handle_error(&mut self, result: Result<(), AdderPlayerError>) {
        match result {
            Ok(()) => {}
            Err(e) => {
                match e {
                    InvalidFileType => {}
                    NoFileSelected => {}
                    // AdderPlayerError::SourceError(VideoError(
                    //     video_rs_adder_dep::Error::ReadExhausted,
                    // )) => {
                    //     let mut state = self.transcoder_state.clone();
                    //     self.source
                    //         .as_mut()
                    //         .unwrap()
                    //         .get_video_mut()
                    //         .state
                    //         .in_interval_count = 0;
                    //     state.core_params.output_path = None;
                    //     self.state_update(state, true)
                    //         .expect("Error creating new transcoder");
                    //     return;
                    // }
                    // AdderTranscoderError::SourceError(_) => {}
                    // AdderTranscoderError::IoError(_) => {}
                    // AdderTranscoderError::OtherError(_) => {}
                    Uninitialized => {}
                    AdderPlayerError::CodecError(_) => {}
                }

                match self.msg_tx.try_send(PlayerInfoMsg::Error(e.to_string())) {
                    Err(TrySendError::Full(..)) => {
                        dbg!(e);
                        eprintln!("Msg channel full");
                    }
                    _ => {}
                };
            }
        }
    }
    fn state_update(
        &mut self,
        player_state: PlayerState,
        force_new: bool,
    ) -> Result<(), AdderPlayerError> {
        dbg!(player_state.core_params.clone());
        dbg!(self.player_state.core_params.clone());
        if force_new || player_state.core_params != self.player_state.core_params {
            eprintln!("Create new transcoder");

            let res = self.core_state_update(player_state);
            if res.is_ok() {
                // Send a message with the frame length of the reconstructed sequence
                let framer_state = &self.framer.as_ref().unwrap().state;
                let frame_length =
                    Duration::from_secs_f64(framer_state.tpf as f64 / framer_state.tps as f64);

                match self
                    .msg_tx
                    .try_send(PlayerInfoMsg::FrameLength(frame_length))
                {
                    Ok(_) => {}
                    Err(TrySendError::Full(..)) => {
                        eprintln!("Metrics channel full");
                    }
                    Err(e) => {
                        panic!("todo");
                    }
                };

                // Send a message with the plane size of the video
                // let plane = self
                //     .source
                //     .as_ref()
                //     .unwrap()
                //     .get_video_ref()
                //     .state
                //     .plane
                //     .clone();
                // match self
                //     .msg_tx
                //     .try_send(TranscoderInfoMsg::Plane((plane, force_new)))
                // {
                //     Ok(_) => {}
                //     Err(TrySendError::Full(..)) => {
                //         eprintln!("Metrics channel full");
                //     }
                //     Err(e) => {
                //         panic!("todo");
                //     }
                // };
            } else {
                eprintln!("Error creating new transcoder: {:?}", res);
                self.framer = None;
                self.player_state = PlayerState::default();
                return res;
            }
            return res;
        } else if player_state.adaptive_params != self.player_state.adaptive_params {
            eprintln!("Modify existing player");
            self.update_params(player_state);
            return self.adaptive_state_update();
        } else {
            eprintln!("No change in transcoder state");
        }
        self.update_params(player_state);

        Ok(())
    }

    fn update_params(&mut self, player_state: PlayerState) {
        self.player_state = player_state;
    }

    fn adaptive_state_update(&mut self) -> Result<(), AdderPlayerError> {
        let source = self.framer.as_mut().ok_or(Uninitialized)?;

        let params = &self.player_state.adaptive_params;

        source.buffer_limit = params.buffer_limit;
        source.state.view_mode(params.view_mode);
        source.detect_features(params.detect_features);

        // source.get_video_mut().update_detect_features(
        //     params.detect_features,
        //     params.show_features,
        //     params.feature_cluster,
        // );

        Ok(())
    }

    fn core_state_update(&mut self, player_state: PlayerState) -> Result<(), AdderPlayerError> {
        self.total_events = 0;
        match &player_state.core_params.input_path_buf_0 {
            None => return Err(NoFileSelected),
            Some(input_path_buf) => match input_path_buf.extension() {
                None => return Err(InvalidFileType),
                Some(ext) => match ext.to_ascii_lowercase().to_str().unwrap() {
                    "adder" => {
                        // adder video
                        let input_path =
                            input_path_buf.to_str().expect("Invalid string").to_string();
                        let (stream, bitreader) = open_file_decoder(&input_path)?;

                        let meta = *stream.meta();

                        let mut reconstructed_frame_rate =
                            meta.tps as f32 / meta.ref_interval as f32;
                        if !is_framed(meta.source_camera) {
                            reconstructed_frame_rate = 60.0;
                        }

                        reconstructed_frame_rate /= player_state.core_params.playback_speed;

                        let framer_builder: FramerBuilder = FramerBuilder::new(meta.plane, 1)
                            .codec_version(meta.codec_version, meta.time_mode)
                            .time_parameters(
                                meta.tps,
                                meta.ref_interval,
                                meta.delta_t_max,
                                Some(reconstructed_frame_rate),
                            )
                            .view_mode(player_state.adaptive_params.view_mode)
                            .mode(INSTANTANEOUS)
                            .buffer_limit(player_state.adaptive_params.buffer_limit)
                            .detect_features(player_state.adaptive_params.detect_features)
                            .source(stream.get_source_type(), meta.source_camera);

                        let mut frame_sequence: FrameSequence<u8> = framer_builder.clone().finish();
                        self.framer = Some(frame_sequence);

                        let mut stream = InputStream {
                            decoder: stream,
                            bitreader,
                        };
                        self.input_stream = Some(stream);
                        self.running_frame = Frame::zeros((
                            meta.plane.h_usize(),
                            meta.plane.w_usize(),
                            meta.plane.c_usize(),
                        ));

                        eprintln!("Created framer");
                    }
                    // "aedat4" | "sock" => {
                    //     // Davis video
                    // }
                    // "dat" => {
                    //     // Prophesee video
                    // }
                    _ => return Err(InvalidFileType),
                },
            },
        }
        // Update the params to match
        self.player_state.core_params = player_state.core_params;
        Ok(())
    }
    async fn consume(
        &mut self,
        // stream: &mut InputStream,
        // frame_sequence: &mut FrameSequence<u8>,
    ) -> Result<(), AdderPlayerError> {
        let stream = self.input_stream.as_mut().ok_or(Uninitialized)?;
        let frame_sequence = self.framer.as_mut().ok_or(Uninitialized)?;

        let mut event_count = 0;

        // let image_mat = frame_sequence.get_frame();
        // let color = image_mat.shape()[2] == 3;
        // let width = image_mat.shape()[1];
        // let height = image_mat.shape()[0];

        let is_color = self.running_frame.shape()[2] == 3;
        let color_channels = self.running_frame.shape()[2];
        let width = self.running_frame.shape()[1];
        let height = self.running_frame.shape()[0];

        while frame_sequence.is_frame_0_filled() {
            let mut idx = 0;
            unsafe {
                let db = self.running_frame.as_slice_mut().unwrap();
                let new_frame = frame_sequence.pop_next_frame().unwrap();
                // Flatten the frame
                for chunk in 0..new_frame.len() {
                    for y in 0..new_frame[chunk].shape()[0] {
                        for x in 0..new_frame[chunk].shape()[1] {
                            for c in 0..new_frame[chunk].shape()[2] {
                                if let Some(val) = new_frame[chunk].uget((y, x, c)) {
                                    db[idx] = *val;
                                }
                                idx += 1;
                            }
                        }
                    }
                }

                // TODO: Reenable the below
                if let Some(feature_interval) = frame_sequence.pop_features() {
                    for feature in feature_interval.features {
                        // let db = display_mat.as_slice_mut().unwrap();

                        let color: u8 = 255;
                        let radius = 2;
                        for i in -radius..=radius {
                            let idx =
                                ((feature.y as i32 + i) * width as i32 * color_channels as i32
                                    + (feature.x as i32) * color_channels as i32)
                                    as usize;
                            db[idx] = color;

                            if is_color {
                                db[idx + 1] = color;
                                db[idx + 2] = color;
                            }

                            let idx = (feature.y as i32 * width as i32 * color_channels as i32
                                + (feature.x as i32 + i) * color_channels as i32)
                                as usize;
                            db[idx] = color;

                            if is_color {
                                db[idx + 1] = color;
                                db[idx + 2] = color;
                            }
                        }
                    }
                }
            }

            let image = prep_epaint_image(&self.running_frame, is_color, width, height).unwrap();

            // self.stream_state.current_t_ticks += frame_sequence.state.tpf;

            // let image_mat = self.display_frame.clone();
            // let color = image_mat.shape()[2] == 3;
            //
            // let image_bevy = prep_bevy_image(image_mat, color, meta.plane.w(), meta.plane.h())?;

            // Set the image to the handle, so that the UI can display it
            // TODO: Actually send the images on a channel, so they can be displayed separately from the decompression thread
            self.image_tx.send(image).await.unwrap();
            // self.player_state.last_frame_display_time = Some(Instant::now());

            // return Ok(());
        }
        let meta = stream.decoder.meta();

        let mut last_event: Option<Event> = None;
        loop {
            // eprintln!("Consume");
            match stream.decoder.digest_event(&mut stream.bitreader) {
                Ok(mut event) => {
                    event_count += 1;
                    let filled = frame_sequence.ingest_event(&mut event, last_event);

                    last_event = Some(event);

                    if filled {
                        // return Ok((event_count, image_bevy));
                        return Ok(());
                    }
                }
                Err(e) => {
                    todo!("handle codec error (e.g., restart playback)");
                    // if !frame_sequence.flush_frame_buffer() {
                    //     eprintln!("Player error: {}", e);
                    //     eprintln!("Completely done");
                    //     // TODO: Need to reset the UI event count events_ppc count when looping back here
                    //     // Loop/restart back to the beginning
                    //     if stream.decoder.get_compression_type() == EncoderType::Raw {
                    //         stream.decoder.set_input_stream_position(
                    //             &mut stream.bitreader,
                    //             meta.header_size as u64,
                    //         )?;
                    //     } else {
                    //         stream
                    //             .decoder
                    //             .set_input_stream_position(&mut stream.bitreader, 1)?;
                    //     }
                    //
                    //     frame_sequence =
                    //         self.framer_builder.clone().map(|builder| builder.finish());
                    //     self.stream_state.last_timestamps = Array::zeros((
                    //         meta.plane.h_usize(),
                    //         meta.plane.w_usize(),
                    //         meta.plane.c_usize(),
                    //     ));
                    //     self.stream_state.current_t_ticks = 0;
                    //     self.current_frame = 0;
                    //
                    //     return Err(Box::try_from(CodecError::Eof).unwrap());
                    // } else {
                    //     return self.consume_source_accurate();
                    // }
                }
            }
        }
    }
}

// TODO: allow flexibility with decoding non-file inputs
pub struct InputStream {
    pub(crate) decoder: Decoder<BufReader<File>>,
    pub(crate) bitreader: BitReader<BufReader<File>, BigEndian>,
}
unsafe impl Send for InputStream {}
