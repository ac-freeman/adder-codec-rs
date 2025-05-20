use crate::framer::scale_intensity::{FrameValue, SaeTime};
use crate::transcoder::source::video::FramedViewMode::SAE;
use crate::transcoder::source::video::{
    integrate_for_px, Source, SourceError, Video, VideoBuilder,
};
use crate::utils::cv::mid_clamp_u8;
use crate::utils::viz::ShowFeatureMode;
use adder_codec_core::codec::{EncoderOptions, EncoderType};
use adder_codec_core::Mode::Continuous;
use adder_codec_core::{
    DeltaT, Event, PixelMultiMode, PlaneSize, SourceCamera, SourceType, TimeMode,
};
use ndarray::Array3;
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::PathBuf;
use video_rs_adder_dep::Frame;

/// The temporal granularity of the source (ticks per second)
const PROPHESEE_SOURCE_TPS: u32 = 1000000;

/// Attributes of a framed video -> ADΔER transcode
pub struct Prophesee<W: Write + std::marker::Send + std::marker::Sync + 'static> {
    pub(crate) video: Video<W>,

    input_reader: BufReader<File>,

    running_t: u32,

    t_subtract: u32,

    /// The timestamp (in-camera) of the last DVS event integrated for each pixel
    pub dvs_last_timestamps: Array3<u32>,

    /// The log-space last intensity value for each pixel
    pub dvs_last_ln_val: Array3<f64>,

    camera_theta: f64,
}

/// A DVS-style contrast event
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DvsEvent {
    t: u32,
    x: u16,
    y: u16,
    p: u8,
}

unsafe impl<W: Write + std::marker::Send + std::marker::Sync + 'static> Sync for Prophesee<W> {}

impl<W: Write + std::marker::Send + std::marker::Sync + 'static> Prophesee<W> {
    /// Create a new `Prophesee` transcoder
    pub fn new(ref_time: u32, input_filename: String) -> Result<Self, Box<dyn Error>> {
        let source = File::open(PathBuf::from(input_filename))?;
        let mut input_reader = BufReader::new(source);

        // Parse header
        let (_, _, _, size) = parse_header(&mut input_reader).unwrap();

        let plane = PlaneSize::new(size.1 as u16, size.0 as u16, 1)?;

        let mut video = Video::new(plane, Continuous, None)?
            .chunk_rows(1)
            // Override the tps to assume the source has a temporal granularity of 1000000/second
            // The `ref_time` in this case scales up the temporal granularity of the source.
            // For example, with ref_time = 20, a timestamp of 12 in the source becomes 240
            // ADDER ticks
            .time_parameters(
                ref_time * PROPHESEE_SOURCE_TPS,
                ref_time,
                ref_time * 2,
                Some(TimeMode::AbsoluteT),
            )?;

        let start_intensities = vec![128_u8; video.state.plane.volume()];
        video.state.running_intensities = Array3::from_shape_vec(
            (plane.h().into(), plane.w().into(), plane.c().into()),
            start_intensities,
        )?;
        video.display_frame_features = video.state.running_intensities.clone();

        let timestamps = vec![2_u32; video.state.plane.volume()];

        let dvs_last_timestamps: Array3<u32> = Array3::from_shape_vec(
            (plane.h().into(), plane.w().into(), plane.c().into()),
            timestamps,
        )?;

        let plane = &video.state.plane;

        let start_vals = vec![(128.0_f64 / 255.0_f64).ln_1p(); video.state.plane.volume()];

        let dvs_last_ln_val: Array3<f64> = Array3::from_shape_vec(
            (plane.h() as usize, plane.w() as usize, plane.c() as usize),
            start_vals,
        )?;

        let prophesee_source = Self {
            video,
            input_reader,
            running_t: 0,
            t_subtract: 0,
            dvs_last_timestamps,
            dvs_last_ln_val,
            camera_theta: 0.02, // A fixed assumption
        };

        Ok(prophesee_source)
    }
}

impl<W: Write + std::marker::Send + std::marker::Sync + 'static> Source<W> for Prophesee<W> {
    fn consume(&mut self) -> Result<Vec<Vec<Event>>, SourceError> {
        if self.running_t == 0 {
            self.video.integrate_matrix(
                self.video.state.running_intensities.clone(),
                self.video.state.params.ref_time as f32,
            )?;
            let first_events: Vec<Event> = self
                .video
                .integrate_matrix(
                    self.video.state.running_intensities.clone(),
                    self.video.state.params.ref_time as f32,
                )?
                .into_iter()
                .flatten()
                .collect();
            assert_eq!(first_events.len(), self.video.state.plane.volume());
            self.running_t = 2;
        }

        // TODO hardcoded: scale the view interval to be 60 FPS GUI display
        let view_interval = PROPHESEE_SOURCE_TPS / 60;

        // Read events from the source file until we find a timestamp that exceeds our `running_t`
        // by at least `view_interval`
        let mut dvs_events: Vec<DvsEvent> = Vec::new();
        let mut dvs_event;
        let start_running_t = self.running_t;
        loop {
            // TODO: integrate to fill in the rest of time once the eof is reached

            dvs_event = match decode_event(&mut self.input_reader) {
                Ok(mut dvs_event) => {
                    // if self.running_t == 2 && dvs_events.is_empty() {
                    //     self.t_subtract = dvs_event.t;
                    //     eprintln!("t_subtract: {}", self.t_subtract);
                    // }

                    dvs_event.t -= self.t_subtract;

                    if dvs_event.t > self.running_t {
                        self.running_t = dvs_event.t;
                    }
                    dvs_event
                }
                Err(e) => {
                    dbg!("End of input file");
                    end_events(self);
                    return Err(e.into());
                }
            };
            dvs_events.push(dvs_event);
            if dvs_events.last().unwrap().t > start_running_t + view_interval {
                break;
            }
        }

        let mut events: Vec<Event> = Vec::new();
        let crf_parameters = *self.video.encoder.options.crf.get_parameters();

        // For every dvs event in our queue, integrate the previously seen intensity for all the
        // time between the pixel's last input and the current event
        for dvs_event in dvs_events {
            let x = dvs_event.x as usize;
            let y = dvs_event.y as usize;
            let p = dvs_event.p as usize;
            let t = dvs_event.t;

            // Get the last timestamp for this pixel
            let last_t = self.dvs_last_timestamps[[y, x, 0]];

            if t < last_t {
                // dbg!("skipping event");
                continue;
            }

            // Get the last ln intensity for this pixel
            let mut last_ln_val = self.dvs_last_ln_val[[y, x, 0]];

            let px = &mut self.video.event_pixel_trees[[y, x, 0]];

            if t > last_t + 1 {
                // Convert the ln intensity to a linear intensity
                let mut last_val = (last_ln_val.exp() - 1.0) * 255.0;

                mid_clamp_u8(&mut last_val, &mut last_ln_val);

                // Integrate the last intensity for this pixel over the time since the last event
                let time_spanned = (t - last_t - 1) * self.video.state.params.ref_time;
                let intensity_to_integrate = last_val * (t - last_t - 1) as f64;

                let mut base_val = 0;
                let _ = integrate_for_px(
                    px,
                    &mut base_val,
                    last_val as u8,
                    intensity_to_integrate as f32,
                    time_spanned as f32,
                    &mut events,
                    &self.video.state.params,
                    &crf_parameters,
                    false,
                );
            }

            // Get the new ln intensity
            let mut new_ln_val = match p {
                0 => last_ln_val - self.camera_theta,
                1 => last_ln_val + self.camera_theta,
                _ => panic!("Invalid polarity"),
            };

            // Update the last intensity for this pixel
            self.dvs_last_ln_val[[y, x, 0]] = new_ln_val;

            // Update the last timestamp for this pixel
            self.dvs_last_timestamps[[y, x, 0]] = t;

            if t > last_t {
                let mut new_val = (new_ln_val.exp() - 1.0) * 255.0;

                mid_clamp_u8(&mut new_val, &mut new_ln_val);

                // Update the last intensity for this pixel
                self.dvs_last_ln_val[[y, x, 0]] = new_ln_val;

                // Integrate 1 source time unit of the new intensity
                let time_spanned = self.video.state.params.ref_time;
                let intensity_to_integrate = new_val;

                let mut base_val = 0;
                let _ = integrate_for_px(
                    px,
                    &mut base_val,
                    new_val as u8,
                    intensity_to_integrate as f32,
                    time_spanned as f32,
                    &mut events,
                    &self.video.state.params,
                    &crf_parameters,
                    false,
                );
            }

            // Update the running intensity for this pixel
            if let Some(event) = px.arena[0].best_event {
                self.video.state.running_intensities[[y, x, 0]] = u8::get_frame_value(
                    &event.into(),
                    SourceType::U8,
                    self.video.state.params.ref_time as f64,
                    32.0,
                    self.video.state.params.delta_t_max,
                    self.video.instantaneous_view_mode,
                    if self.video.instantaneous_view_mode == SAE {
                        Some(SaeTime {
                            running_t: px.running_t as DeltaT,
                            last_fired_t: px.last_fired_t as DeltaT,
                        })
                    } else {
                        None
                    },
                );
                self.video.display_frame_features[[y, x, 0]] =
                    self.video.state.running_intensities[[y, x, 0]];
            };
        }

        if self.video.state.feature_detection {
            self.video.display_frame_features = self.video.state.running_intensities.clone();
        }

        // It's expected that the function will spatially parallelize the integrations. With sparse
        // data, though, this could be pretty wasteful. For now, just wrap the vec in another vec.
        let events_nested: Vec<Vec<Event>> = vec![events];

        self.video.handle_features(&events_nested)?;

        for events in &events_nested {
            for event in events {
                self.video.encoder.ingest_event(*event)?;
            }
        }

        Ok(events_nested)
    }

    fn crf(&mut self, crf: u8) {
        self.video.update_crf(crf);
    }

    fn get_video_mut(&mut self) -> &mut Video<W> {
        &mut self.video
    }

    fn get_video_ref(&self) -> &Video<W> {
        &self.video
    }

    fn get_video(self) -> Video<W> {
        self.video
    }

    fn get_input(&self) -> Option<&Frame> {
        None
    }

    fn get_running_input_bitrate(&self) -> f64 {
        // TODO
        0.0
    }
}

fn end_events<W: Write + std::marker::Send + std::marker::Sync + 'static>(
    prophesee: &mut Prophesee<W>,
) {
    let mut events: Vec<Event> = Vec::new();
    let crf_parameters = *prophesee.video.encoder.options.crf.get_parameters();

    for y in 0..prophesee.video.state.plane.h_usize() {
        for x in 0..prophesee.video.state.plane.w_usize() {
            let px = &mut prophesee.video.event_pixel_trees[[y, x, 0]];
            let mut base_val = 0;

            // Get the last ln intensity for this pixel
            let last_ln_val = prophesee.dvs_last_ln_val[[y, x, 0]];

            // Convert the ln intensity to a linear intensity
            let last_val = (last_ln_val.exp() - 1.0) * 255.0;

            assert!(prophesee.running_t - prophesee.dvs_last_timestamps[[y, x, 0]] > 0);

            // Integrate the last intensity for this pixel over the time since the last event
            let time_spanned = (prophesee.running_t - prophesee.dvs_last_timestamps[[y, x, 0]])
                * prophesee.video.state.params.ref_time;
            let intensity_to_integrate = last_val * time_spanned as f64;

            let _ = integrate_for_px(
                px,
                &mut base_val,
                last_val as u8,
                intensity_to_integrate as f32,
                time_spanned as f32,
                &mut events,
                &prophesee.video.state.params,
                &crf_parameters,
                false,
            );
        }
    }

    for event in &events {
        prophesee.video.encoder.ingest_event(*event).unwrap();
    }
}

fn parse_header(file: &mut BufReader<File>) -> io::Result<(u64, u8, u8, (u32, u32))> {
    file.seek(SeekFrom::Start(0))?; // Seek to the beginning of the file
    let mut bod = 0;
    let mut end_of_header = false;
    let mut num_comment_line = 0;
    let mut size = [None, None];

    // Parse header
    while !end_of_header {
        bod = file.stream_position()?; // Get the current position
        let mut line = Vec::new(); // Change to Vec<u8>
        file.read_until(b'\n', &mut line)?; // Read until newline as binary data
        if line.is_empty() || line[0] != b'%' {
            end_of_header = true;
        } else {
            let words: Vec<&[u8]> = line.split(|&x| x == b' ' || x == b'\t').collect(); // Use &[u8] instead of &str

            if words.len() > 1 {
                match words[1] {
                    b"Height" => {
                        size[0] = line_to_hw(words);
                    }
                    b"Width" => {
                        size[1] = line_to_hw(words);
                    }
                    _ => {}
                }
            }
            num_comment_line += 1;
        }
    }

    // Parse data
    file.seek(SeekFrom::Start(bod))?; // Seek back to the position after the header
    let (ev_type, ev_size) = if num_comment_line > 0 {
        // Read event type and size
        let mut buf = [0; 2]; // Adjust the buffer size based on your data size
        file.read_exact(&mut buf)?;
        let ev_type = buf[0];
        let ev_size = buf[1];
        if ev_size != 8 || (ev_type != 0 && ev_type != 12) {
            panic!("Invalid Prophesee event size");
        }

        (ev_type, ev_size)
    } else {
        (0, 0) // Placeholder values, replace with actual logic
    };
    bod = file.stream_position()?;
    Ok((
        bod,
        ev_type,
        ev_size,
        (size[0].unwrap_or(70), size[1].unwrap_or(100)),
    ))
}

fn line_to_hw(words: Vec<&[u8]>) -> Option<u32> {
    let word = words.get(2).unwrap();
    let new_word = if *word.last().unwrap() == b'\n' {
        // Remove the trailing newline
        &word[..word.len() - 1]
    } else {
        *word
    };
    std::str::from_utf8(new_word)
        .ok()
        .and_then(|s| s.parse().ok())
}

fn decode_event(reader: &mut BufReader<File>) -> io::Result<DvsEvent> {
    // Read one record
    let mut buffer = [0; 8]; // Adjust this size to match your record size
    reader.read_exact(&mut buffer)?;

    // Interpret the bytes as 't' and 'data'
    let t = u32::from_le_bytes([buffer[0], buffer[1], buffer[2], buffer[3]]);
    let data = i32::from_le_bytes([buffer[4], buffer[5], buffer[6], buffer[7]]);

    // Perform bitwise operations
    let x = (data & 0x3FF) as u16; // All but last 14 bits
    let y = ((data & 0xFFFC000) >> 14) as u16; // All but second-to-last grouping of 14 bits
    let p = ((data & 0x10000000) >> 28) as u8; // Just the 4th bit

    Ok(DvsEvent { t, x, y, p })
}

impl<W: Write + std::marker::Send + std::marker::Sync + 'static> VideoBuilder<W> for Prophesee<W> {
    fn crf(mut self, crf: u8) -> Self {
        self.video.update_crf(crf);
        self
    }

    fn quality_manual(
        mut self,
        c_thresh_baseline: u8,
        c_thresh_max: u8,
        delta_t_max_multiplier: u32,
        c_increase_velocity: u8,
        feature_c_radius_denom: f32,
    ) -> Self {
        self.video.update_quality_manual(
            c_thresh_baseline,
            c_thresh_max,
            delta_t_max_multiplier,
            c_increase_velocity,
            feature_c_radius_denom,
        );
        self
    }

    fn chunk_rows(mut self, chunk_rows: usize) -> Self {
        self.video = self.video.chunk_rows(chunk_rows);
        self
    }

    fn time_parameters(
        mut self,
        tps: DeltaT,
        ref_time: DeltaT,
        delta_t_max: DeltaT,
        time_mode: Option<TimeMode>,
    ) -> Result<Self, SourceError> {
        eprintln!("setting dtref to {}", ref_time);
        self.video = self
            .video
            .time_parameters(tps, ref_time, delta_t_max, time_mode)?;
        Ok(self)
    }

    fn write_out(
        mut self,
        source_camera: SourceCamera,
        time_mode: TimeMode,
        pixel_multi_mode: PixelMultiMode,
        adu_interval: Option<usize>,
        encoder_type: EncoderType,
        encoder_options: EncoderOptions,
        write: W,
    ) -> Result<Box<Self>, SourceError> {
        self.video = self.video.write_out(
            Some(source_camera),
            Some(time_mode),
            Some(pixel_multi_mode),
            adu_interval,
            encoder_type,
            encoder_options,
            write,
        )?;
        Ok(Box::new(self))
    }
    fn detect_features(mut self, detect_features: bool, show_features: ShowFeatureMode) -> Self {
        self.video = self.video.detect_features(detect_features, show_features);
        self
    }

    #[cfg(feature = "feature-logging")]
    fn log_path(self, _name: String) -> Self {
        todo!()
    }
}
