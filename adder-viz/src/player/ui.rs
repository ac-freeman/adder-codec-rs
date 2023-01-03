use adder_codec_rs::framer::driver::Framer;
use adder_codec_rs::framer::scale_intensity::event_to_intensity;
use adder_codec_rs::{Codec, SourceCamera};
use std::error::Error;
use std::time::Duration;

use adder_codec_rs::transcoder::source::video::FramedViewMode;
use bevy::asset::Assets;
use bevy::ecs::system::Resource;
use bevy::prelude::{Commands, Image, Res, ResMut};
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::time::Time;
use bevy_egui::egui::{Color32, RichText, Ui};

use crate::player::adder::AdderPlayer;
use crate::{add_checkbox_row, add_radio_row, add_slider_row, Images};
use bevy_egui::egui;
use opencv::core::{Mat, MatTraitConstManual, MatTraitManual};
use opencv::imgproc;
use rayon::current_num_threads;

#[derive(PartialEq)]
pub struct PlayerUiSliders {
    playback_speed: f32,
    thread_count: usize,
}

impl Default for PlayerUiSliders {
    fn default() -> Self {
        Self {
            playback_speed: 1.0,
            thread_count: 4,
        }
    }
}

#[derive(PartialEq, Clone)]
enum ReconstructionMethod {
    Fast,
    Accurate,
}

pub struct PlayerUiState {
    playing: bool,
    looping: bool,
    view_mode: FramedViewMode,
    reconstruction_method: ReconstructionMethod,
    current_frame: u32,
    total_frames: u32,
    current_time: f32,
    total_time: f32,
}

impl Default for PlayerUiState {
    fn default() -> Self {
        Self {
            playing: true,
            looping: true,
            view_mode: FramedViewMode::Intensity,
            reconstruction_method: ReconstructionMethod::Accurate,
            current_frame: 0,
            total_frames: 0,
            current_time: 0.0,
            total_time: 0.0,
        }
    }
}

pub struct InfoUiState {
    events_per_sec: f64,
    events_ppc_per_sec: f64,
    events_ppc_total: f64,
    events_total: u64,
    source_name: RichText,
}

impl Default for InfoUiState {
    fn default() -> Self {
        InfoUiState {
            events_per_sec: 0.,
            events_ppc_per_sec: 0.,
            events_ppc_total: 0.0,
            events_total: 0,
            source_name: RichText::new("No file selected yet"),
        }
    }
}

impl InfoUiState {
    fn clear_stats(&mut self) {
        self.events_per_sec = 0.;
        self.events_ppc_per_sec = 0.;
        self.events_ppc_total = 0.0;
        self.events_total = 0;
    }
}

#[derive(Resource, Default)]
pub struct PlayerState {
    player: AdderPlayer,
    ui_state: PlayerUiState,
    ui_sliders: PlayerUiSliders,
    ui_sliders_drag: PlayerUiSliders,
    ui_info_state: InfoUiState,
}

impl PlayerState {
    // Fill in the side panel with sliders for playback speed and buttons for play/pause/stop
    pub fn side_panel_ui(
        &mut self,
        ui: &mut Ui,
        mut commands: Commands,
        _images: &mut ResMut<Assets<Image>>,
    ) {
        ui.horizontal(|ui| {
            ui.heading("ADΔER Parameters");
            if ui.add(egui::Button::new("Reset params")).clicked() {
                self.ui_state = Default::default();
                self.ui_sliders = Default::default();
                if self.ui_sliders_drag != self.ui_sliders {
                    self.reset_update_adder_params()
                }
                self.ui_sliders_drag = Default::default();
            }
            if ui.add(egui::Button::new("Reset video")).clicked() {
                self.player = AdderPlayer::default();
                self.ui_state = Default::default();
                self.ui_sliders = Default::default();
                self.ui_sliders_drag = Default::default();
                self.ui_info_state = Default::default();
                self.reset_update_adder_params();
                commands.insert_resource(Images::default());
            }
        });
        egui::Grid::new("my_grid")
            .num_columns(2)
            .spacing([10.0, 4.0])
            .striped(true)
            .show(ui, |ui| {
                self.side_panel_grid_contents(ui);
            });
    }

    pub fn side_panel_grid_contents(&mut self, ui: &mut Ui) {
        let mut need_to_update = add_slider_row(
            true,
            true,
            "Playback speed:",
            ui,
            &mut self.ui_sliders.playback_speed,
            &mut self.ui_sliders_drag.playback_speed,
            0.1..=15.0,
            vec![1.0, 2.0, 5.0, 10.0],
            0.1,
        );

        match &self.player.input_stream {
            None => {}
            Some(stream) => {
                let duration = Duration::from_nanos(
                    ((self.player.current_t_ticks as f64 / stream.tps as f64) * 1.0e9) as u64,
                );
                ui.add_enabled(true, egui::Label::new("Current time:"));
                ui.add_enabled(true, egui::Label::new(to_string(duration)));
                ui.end_row();
            }
        }

        ui.add_enabled(true, egui::Label::new("Playback controls:"));
        ui.horizontal(|ui| {
            if self.ui_state.playing {
                if ui.button("⏸").clicked() {
                    self.ui_state.playing = false;
                }
            } else if ui.button("▶").clicked() {
                self.ui_state.playing = true;
            }
            // TODO: remove this?
            if ui.button("⏹").clicked() {
                self.ui_state.playing = false;
                need_to_update = true;
            }

            if ui.button("⏮").clicked() {
                self.ui_state.playing = true;
                need_to_update = true;
            }
        });
        ui.end_row();

        // TODO: decoding is single-threaded for now
        add_slider_row(
            false,
            false,
            "Thread count:",
            ui,
            &mut self.ui_sliders.thread_count,
            &mut self.ui_sliders_drag.thread_count,
            1..=(current_num_threads() - 1).max(4),
            vec![],
            1,
        );
        add_checkbox_row(
            true,
            "Loop:",
            "Loop playback?",
            ui,
            &mut self.ui_state.looping,
        ); // TODO: add more sliders

        // TODO
        need_to_update |= add_radio_row(
            true,
            "View mode:",
            vec![
                ("Intensity", FramedViewMode::Intensity),
                ("D", FramedViewMode::D),
                ("Δt", FramedViewMode::DeltaT),
            ],
            ui,
            &mut self.ui_state.view_mode,
        );
        need_to_update |= add_radio_row(
            true,
            "Reconstruction method:",
            vec![
                ("Fast", ReconstructionMethod::Fast),
                ("Accurate", ReconstructionMethod::Accurate),
            ],
            ui,
            &mut self.ui_state.reconstruction_method,
        );

        if need_to_update {
            self.reset_update_adder_params()
        }
    }

    pub fn consume_source(
        &mut self,
        images: ResMut<Assets<Image>>,
        handles: ResMut<Images>,
        commands: Commands,
    ) -> Result<(), Box<dyn Error>> {
        if !self.ui_state.playing {
            return Ok(());
        }

        let stream = match &mut self.player.input_stream {
            None => {
                return Ok(());
            }
            Some(s) => s,
        };

        // Reset the stats if we're starting a new looped playback of the video
        if let Ok(pos) = stream.get_input_stream_position() {
            if pos == stream.header_size as u64 {
                match &mut self.player.frame_sequence {
                    None => { // TODO: error
                    }
                    Some(frame_sequence) => {
                        frame_sequence.frames_written = 0;
                    }
                };
                self.ui_info_state.clear_stats();
                self.ui_state.current_time = 0.0;
                self.ui_state.total_time = 0.0;
                self.ui_state.current_frame = 0;
                self.ui_state.total_frames = 0;
                self.player.current_t_ticks = 0;
            }
        }

        match self.ui_state.reconstruction_method {
            ReconstructionMethod::Fast => {
                self.consume_source_fast(images, handles, commands)?;
            }
            ReconstructionMethod::Accurate => {
                self.consume_source_accurate(images, handles, commands)?;
            }
        }
        Ok(())
    }

    fn consume_source_fast(
        &mut self,
        mut images: ResMut<Assets<Image>>,
        mut handles: ResMut<Images>,
        _commands: Commands,
    ) -> Result<(), Box<dyn Error>> {
        if self.ui_state.current_frame == 0 {
            self.ui_state.current_frame = 1; // TODO: temporary hack
        }
        if !self.ui_state.playing {
            return Ok(());
        }
        let stream = match &mut self.player.input_stream {
            None => {
                return Ok(());
            }
            Some(s) => s,
        };

        let _frame_sequence = match &mut self.player.frame_sequence {
            None => {
                return Ok(());
            }
            Some(s) => s,
        };

        let frame_length = stream.ref_interval as f64 * self.ui_sliders.playback_speed as f64; //TODO: temp
        {
            let display_mat = &mut self.player.display_mat;

            loop {
                if self.player.current_t_ticks as u128
                    > (self.ui_state.current_frame as u128 * frame_length as u128)
                {
                    self.ui_state.current_frame += 1;
                    break;
                }

                match stream.decode_event() {
                    Ok(event) if event.d <= 0xFE => {
                        // event_count += 1;
                        let y = event.coord.y as i32;
                        let x = event.coord.x as i32;
                        let c = event.coord.c.unwrap_or(0) as i32;
                        if (y | x | c) == 0x0 {
                            self.player.current_t_ticks += event.delta_t;
                        }

                        let frame_intensity = (event_to_intensity(&event)
                            * stream.ref_interval as f64)
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
                        // unsafe {
                        //     let px: &mut u8 = display_mat.at_3d_unchecked_mut(y, x, c).unwrap();
                        //     *px = frame_intensity as u8;
                        // }
                    }
                    Err(_e) => {
                        match stream.set_input_stream_position(stream.header_size as u64) {
                            Ok(_) => {}
                            Err(ee) => {
                                eprintln!("{}", ee)
                            }
                        };
                        self.player.frame_sequence = self
                            .player
                            .framer_builder
                            .clone()
                            .map(|builder| builder.finish());
                        if !self.ui_state.looping {
                            self.ui_state.playing = false;
                        }
                        self.player.current_t_ticks = 0;
                        return Ok(());
                    }
                    _ => {}
                }
            }
        }

        let mut image_mat_bgra = Mat::default();
        imgproc::cvt_color(
            &self.player.display_mat,
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
        self.player.live_image = image_bevy;

        let handle = images.add(self.player.live_image.clone());
        handles.image_view = handle;
        Ok(())
    }

    pub fn consume_source_accurate(
        &mut self,
        mut images: ResMut<Assets<Image>>,
        mut handles: ResMut<Images>,
        _commands: Commands,
    ) -> Result<(), Box<dyn Error>> {
        let stream = match &mut self.player.input_stream {
            None => {
                return Ok(());
            }
            Some(s) => s,
        };

        let frame_sequence = match &mut self.player.frame_sequence {
            None => {
                return Ok(());
            }
            Some(s) => s,
        };

        let display_mat = &mut self.player.display_mat;

        if frame_sequence.is_frame_0_filled() {
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
                        println!("Couldn't pop chunk {}!", chunk_num)
                    }
                }
            }
            frame_sequence.frames_written += 1;
            self.player.current_t_ticks += frame_sequence.tpf;

            let mut image_mat_bgra = Mat::default();
            imgproc::cvt_color(display_mat, &mut image_mat_bgra, imgproc::COLOR_BGR2BGRA, 4)?;

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
            self.player.live_image = image_bevy;

            let handle = images.add(self.player.live_image.clone());
            handles.image_view = handle;
        }

        loop {
            match stream.decode_event() {
                Ok(mut event) => {
                    self.ui_info_state.events_total += 1;
                    if frame_sequence.ingest_event(&mut event) {
                        break;
                    }
                }
                Err(_e) => {
                    stream.set_input_stream_position(stream.header_size as u64)?;
                    self.player.frame_sequence = self
                        .player
                        .framer_builder
                        .clone()
                        .map(|builder| builder.finish());
                    if !self.ui_state.looping {
                        self.ui_state.playing = false;
                    }
                    return Ok(());
                }
            }
        }
        Ok(())
    }

    pub fn central_panel_ui(&mut self, ui: &mut Ui, time: Res<Time>) {
        ui.horizontal(|ui| {
            if ui.button("Open file").clicked() {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("adder video", &["adder"])
                    .pick_file()
                {
                    self.replace_player(&path);
                }
            }

            ui.label("OR drag and drop your ADΔER file here (.adder)");
        });

        ui.label(self.ui_info_state.source_name.clone());

        if let Some(stream) = &self.player.input_stream {
            let duration = Duration::from_nanos(
                ((self.player.current_t_ticks as f64 / stream.tps as f64) * 1.0e9) as u64,
            );
            self.ui_info_state.events_per_sec =
                self.ui_info_state.events_total as f64 / duration.as_secs() as f64;
            self.ui_info_state.events_ppc_total =
                self.ui_info_state.events_total as f64 / stream.plane.volume() as f64;
            self.ui_info_state.events_ppc_per_sec =
                self.ui_info_state.events_ppc_total / duration.as_secs() as f64;
        }

        // TODO: make fps accurate and meaningful here
        ui.label(format!(
            "{:.2} transcoded FPS\t\
            {:.2} events per source sec\t\
            {:.2} events PPC per source sec\t\
            {:.0} events total\t\
            {:.0} events PPC total
            ",
            1. / time.delta_seconds(),
            self.ui_info_state.events_per_sec,
            self.ui_info_state.events_ppc_per_sec,
            self.ui_info_state.events_total,
            self.ui_info_state.events_ppc_total
        ));
    }

    fn reset_update_adder_params(&mut self) {
        self.ui_state.current_frame = match self.ui_state.reconstruction_method {
            ReconstructionMethod::Fast => 1,
            ReconstructionMethod::Accurate => 0,
        };
        self.ui_state.total_frames = 0;
        self.ui_state.current_time = 0.0;
        self.ui_state.total_time = 0.0;
        let path_buf = match &self.player.path_buf {
            None => {
                return;
            }
            Some(p) => p,
        };

        match AdderPlayer::new(
            path_buf,
            self.ui_sliders.playback_speed,
            self.ui_state.view_mode,
        ) {
            Ok(player) => self.player = player,
            Err(e) => {
                self.ui_info_state.source_name = RichText::new(e.to_string()).color(Color32::RED);
            }
        }
    }

    pub fn replace_player(&mut self, path_buf: &std::path::Path) {
        match AdderPlayer::new(
            path_buf,
            self.ui_sliders.playback_speed,
            self.ui_state.view_mode,
        ) {
            Ok(player) => {
                self.player = player;
                self.ui_info_state.source_name = RichText::from(match path_buf.to_str() {
                    None => "Error: couldn't get path string".to_string(),
                    Some(path) => path.to_string(),
                })
                .color(Color32::DARK_GREEN);
            }
            Err(e) => {
                self.ui_info_state.source_name = RichText::new(e.to_string()).color(Color32::RED);
            }
        }
        self.ui_state.current_frame = 1;
    }
}

fn to_string(duration: Duration) -> String {
    let hours = duration.as_secs() / 3600;
    let mins = (duration.as_secs() % 3600) / 60;
    let secs = duration.as_secs() % 60;
    let nanos = duration.subsec_nanos();
    format!("{}:{}:{}.{:09}", hours, mins, secs, nanos)
}
