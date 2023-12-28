use std::error::Error;
use std::path::PathBuf;
use ndarray::Array3;
use adder_codec_core::Mode::Continuous;
use adder_codec_core::{DeltaT, Event, PlaneSize, SourceCamera, TimeMode};
use crate::transcoder::source::video::{Source, SourceError, Video, VideoBuilder};
use serde::Deserialize;
use std::fs::File;
use std::io::{self, BufRead, Write, BufReader, Read, Seek, SeekFrom};
use std::path::Path;
use rayon::ThreadPool;
use video_rs_adder_dep::Frame;
use adder_codec_core::codec::{EncoderOptions, EncoderType};
use crate::utils::viz::ShowFeatureMode;

/// Attributes of a framed video -> ADΔER transcode
pub struct Prophesee<W: Write> {
    pub(crate) video: Video<W>,

    time_change: f64,
    num_dvs_events: usize,
}

#[derive(Debug, Deserialize, Clone)]
struct DvsEvent {
    t: u32,
    x: u16,
    y: u16,
    p: u8,
}

unsafe impl<W: Write> Sync for Prophesee<W> {}

impl<W: Write + 'static> Prophesee<W> {
    /// Create a new `Prophesee` transcoder
    pub fn new(
        input_filename: String,
    ) -> Result<Self, Box<dyn Error>> {
        let source = File::open(PathBuf::from(input_filename))?;
        let mut file = BufReader::new(source);

        // Parse header
        let (bod, _, _, size) = parse_header(&mut file).unwrap();

        let plane = PlaneSize::new(size.1 as u16, size.0 as u16, 1)?;

        let video = Video::new(
            plane,

                 Continuous,

            None,
        )?.chunk_rows(1);

        let plane = &video.state.plane;

        let timestamps = vec![0_u32; video.state.plane.volume()];

        let dvs_last_timestamps: Array3<u32> = Array3::from_shape_vec(
            (plane.h().into(), plane.w().into(), plane.c().into()),
            timestamps,
        )?;

        let timestamps = vec![0.0_f64; video.state.plane.volume()];

        let dvs_last_ln_val: Array3<f64> = Array3::from_shape_vec(
            (plane.h() as usize, plane.w() as usize, plane.c() as usize),
            timestamps,
        )?;

        let prophesee_source = Prophesee {
            video,
            time_change: 0.0,
            num_dvs_events: 0,
        };

        Ok(prophesee_source)
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
        bod = file.seek(SeekFrom::Current(0))?; // Get the current position
        let mut line = Vec::new(); // Change to Vec<u8>
        file.read_until(b'\n', &mut line)?; // Read until newline as binary data
        if line.is_empty() || line[0] != b'%' {
            end_of_header = true;
        } else {
            let words: Vec<&[u8]> = line.split(|&x| x == b' ' || x == b'\t').collect(); // Use &[u8] instead of &str

            if words.len() > 1 {
                match words[1] {
                    b"Height" => {
                        size[0] = words.get(2).map(|s| {
                            std::str::from_utf8(s)
                                .ok()
                                .and_then(|s| s.parse().ok())
                        }).flatten();
                    }
                    b"Width" => {
                        size[1] = words.get(2).map(|s| {
                            std::str::from_utf8(s)
                                .ok()
                                .and_then(|s| s.parse().ok())
                        }).flatten();
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

        (ev_type, ev_size)
    } else {
        (0, 0) // Placeholder values, replace with actual logic
    };
    bod = file.seek(SeekFrom::Current(0))?;
    Ok((bod, ev_type, ev_size, (size[0].unwrap_or(320), size[1].unwrap_or(320))))
}

impl<W: Write + 'static + std::marker::Send> Source<W> for Prophesee<W> {
    fn consume(&mut self, view_interval: u32, thread_pool: &ThreadPool) -> Result<Vec<Vec<Event>>, SourceError> {
        todo!()
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
        todo!()
    }
}

impl<W: Write + 'static> VideoBuilder<W> for Prophesee<W> {
    fn contrast_thresholds(mut self, c_thresh_pos: u8, _c_thresh_neg: u8) -> Self {
        self.video = self.video.c_thresh_pos(c_thresh_pos);
        // self.video = self.video.c_thresh_neg(c_thresh_neg);
        self
    }

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

    fn c_thresh_pos(mut self, c_thresh_pos: u8) -> Self {
        self.video = self.video.c_thresh_pos(c_thresh_pos);
        self
    }

    fn c_thresh_neg(self, _c_thresh_neg: u8) -> Self {
        // self.video = self.video.c_thresh_neg(c_thresh_neg);
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
        encoder_type: EncoderType,
        encoder_options: EncoderOptions,
        write: W,
    ) -> Result<Box<Self>, SourceError> {
        self.video = self.video.write_out(
            Some(source_camera),
            Some(time_mode),
            encoder_type,
            encoder_options,
            write,
        )?;
        Ok(Box::new(self))
    }

    fn show_display(mut self, show_display: bool) -> Self {
        self.video = self.video.show_display(show_display);
        self
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