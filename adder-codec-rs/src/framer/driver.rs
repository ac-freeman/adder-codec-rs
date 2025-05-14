use crate::framer::scale_intensity::{FrameValue, SaeTime};
use bincode::config::{BigEndian, FixintEncoding, WithOtherEndian, WithOtherIntEncoding};
use bincode::{DefaultOptions, Options};
use rayon::iter::ParallelIterator;

use std::collections::VecDeque;
use std::error::Error;
use std::fmt;

use adder_codec_core::{
    BigT, Coord, DeltaT, Event, PlaneSize, SourceCamera, SourceType, TimeMode, D_EMPTY,
};
use std::fs::File;
use std::io::BufWriter;

// Want one main framer with the same functions
// Want additional functions
// Want ability to get instantaneous frames at a fixed interval, or at api-spec'd times
// Want ability to get full integration frames at a fixed interval, or at api-spec'd times

/// The mode for how a `Framer` should handle events which span multiple frames and frames
/// spanning multiple events.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum FramerMode {
    /// Each frame's pixel values are derived from only the _last_ event which spanned the
    /// frame's integration period.
    INSTANTANEOUS,

    /// The frame is the sum of all events in the integration period.
    INTEGRATION,
}

/// Builder for a Framer.
#[derive(Clone)]
#[must_use]
pub struct FramerBuilder {
    plane: PlaneSize,
    tps: DeltaT,
    output_fps: Option<f32>,
    mode: FramerMode,
    view_mode: FramedViewMode,
    source: SourceType,
    codec_version: u8,
    source_camera: SourceCamera,
    time_mode: TimeMode,
    ref_interval: DeltaT,
    delta_t_max: DeltaT,
    detect_features: bool,
    buffer_limit: Option<u32>,

    /// The number of rows to process in each chunk (thread).
    pub chunk_rows: usize,
}

impl FramerBuilder {
    /// Create a new [`FramerBuilder`].
    pub fn new(plane: PlaneSize, chunk_rows: usize) -> Self {
        Self {
            plane,
            chunk_rows,
            tps: 150_000,
            output_fps: None,
            mode: FramerMode::INSTANTANEOUS,
            view_mode: FramedViewMode::Intensity,
            source: SourceType::U8,
            codec_version: 3,
            source_camera: SourceCamera::default(),
            time_mode: TimeMode::default(),
            ref_interval: 5000,
            delta_t_max: 5000,
            detect_features: false,
            buffer_limit: None,
        }
    }

    /// Set the time parameters.
    pub fn time_parameters(
        mut self,
        tps: DeltaT,
        ref_interval: DeltaT,
        delta_t_max: DeltaT,
        output_fps: Option<f32>,
    ) -> Self {
        self.tps = tps;
        self.ref_interval = ref_interval;
        self.delta_t_max = delta_t_max;
        self.output_fps = output_fps;
        self
    }

    /// Limit the size of the reconstruction frame buffer (for speed/latency)
    pub fn buffer_limit(mut self, buffer_limit: Option<u32>) -> Self {
        self.buffer_limit = buffer_limit;
        self
    }

    /// Set the framer mode.
    pub fn mode(mut self, mode: FramerMode) -> Self {
        self.mode = mode;
        self
    }

    /// Set the view mode.
    pub fn view_mode(mut self, mode: FramedViewMode) -> Self {
        self.view_mode = mode;
        self
    }

    /// Set the source type and camera.
    pub fn source(mut self, source: SourceType, source_camera: SourceCamera) -> Self {
        self.source_camera = source_camera;
        self.source = source;
        self
    }

    /// Set the codec version and time mode.
    pub fn codec_version(mut self, codec_version: u8, time_mode: TimeMode) -> Self {
        self.codec_version = codec_version;
        self.time_mode = time_mode;
        self
    }

    /// Build a [`Framer`].
    /// TODO: Make this return a result
    #[must_use]
    pub fn finish<T>(self) -> FrameSequence<T>
    where
        T: FrameValue<Output = T>
            + Default
            + Send
            + Serialize
            + Sync
            + std::marker::Copy
            + num_traits::Zero
            + Into<f64>,
    {
        FrameSequence::<T>::new(self)
    }

    /// Set whether to detect features.
    pub fn detect_features(mut self, detect_features: bool) -> Self {
        self.detect_features = detect_features;
        self
    }
}

/// A trait for accumulating ADΔER events into frames.
pub trait Framer {
    /// The type of the output frame.
    type Output;
    /// Create a new [`Framer`] with the given [`FramerBuilder`].
    fn new(builder: FramerBuilder) -> Self;

    /// Ingest an ADΔER event. Will process differently depending on choice of [`FramerMode`].
    ///
    /// If [INSTANTANEOUS](FramerMode::INSTANTANEOUS), this function will set the corresponding output frame's pixel value to
    /// the value derived from this [`Event`], only if this is the first value ingested for that
    /// pixel and frame. Otherwise, the operation will silently be ignored.
    ///
    /// If [INTEGRATION](FramerMode::INTEGRATION), this function will integrate this [`Event`] value for the corresponding
    /// output frame(s)
    fn ingest_event(&mut self, event: &mut Event, last_event: Option<Event>) -> bool;

    /// Ingest a vector of a vector of ADΔER events.
    fn ingest_events_events(&mut self, events: Vec<Vec<Event>>) -> bool;
    /// For all frames left that we haven't written out yet, for any None pixels, set them to the
    /// last recorded intensity for that pixel.
    ///
    /// Returns `true` if there are frames now ready to write out
    fn flush_frame_buffer(&mut self) -> bool;

    fn detect_features(&mut self, detect_features: bool);
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Frame<T> {
    pub(crate) array: Array3<T>,
    pub(crate) filled_count: usize,
}

/// Errors that can occur when working with [`FrameSequence`]
#[derive(Debug)]
pub enum FrameSequenceError {
    /// Frame index out of bounds
    InvalidIndex,

    /// Frame not initialized
    UninitializedFrame,

    /// Frame not initialized
    UninitializedFrameChunk,

    /// An impossible "fill count" encountered
    BadFillCount,
}

impl fmt::Display for FrameSequenceError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            FrameSequenceError::InvalidIndex => write!(f, "Invalid frame index"),
            FrameSequenceError::UninitializedFrame => write!(f, "Uninitialized frame"),
            FrameSequenceError::UninitializedFrameChunk => write!(f, "Uninitialized frame chunk"),
            FrameSequenceError::BadFillCount => write!(f, "Bad fill count"),
        }
    }
}

impl From<FrameSequenceError> for Box<dyn std::error::Error> {
    fn from(value: FrameSequenceError) -> Self {
        value.to_string().into()
    }
}

// impl Display for FrameSequenceError {
//     fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
//         todo!()
//     }
// }
//
// impl std::error::Error for FrameSequenceError {}

/// The state of a [`FrameSequence`]
pub struct FrameSequenceState {
    /// The number of frames written to the output so far
    frames_written: i64,
    plane: PlaneSize,

    /// Ticks per output frame
    pub tpf: DeltaT,

    /// Ticks per second
    pub tps: DeltaT,
    pub(crate) source: SourceType,
    codec_version: u8,
    source_camera: SourceCamera,
    ref_interval: DeltaT,
    source_dtm: DeltaT,
    view_mode: FramedViewMode,
    time_mode: TimeMode,
}

impl FrameSequenceState {
    pub fn reset(&mut self) {
        self.frames_written = 0;
    }

    pub fn view_mode(&mut self, view_mode: FramedViewMode) {
        self.view_mode = view_mode;
    }
}

/// Associates detected features with the source time in which they were detected (since ADDER
/// events may arrive out of order)
pub struct FeatureInterval {
    end_ts: BigT,
    pub features: Vec<Coord>,
}

/// A sequence of frames, each of which is a 3D array of [`FrameValue`]s
#[allow(dead_code)]
pub struct FrameSequence<T> {
    /// The state of the frame sequence
    pub state: FrameSequenceState,
    pub(crate) frames: Vec<VecDeque<Frame<Option<T>>>>,
    pub(crate) frame_idx_offsets: Vec<i64>,
    pub(crate) pixel_ts_tracker: Vec<Array3<BigT>>,
    pub(crate) last_filled_tracker: Vec<Array3<i64>>,
    pub(crate) last_frame_intensity_tracker: Vec<Array3<T>>,
    chunk_filled_tracker: Vec<bool>,
    pub(crate) mode: FramerMode,
    pub(crate) detect_features: bool,
    pub(crate) features: VecDeque<FeatureInterval>,
    pub buffer_limit: Option<u32>,

    pub(crate) running_intensities: Array3<u8>,

    /// Number of rows per chunk (per thread)
    pub chunk_rows: usize,
    bincode: WithOtherEndian<WithOtherIntEncoding<DefaultOptions, FixintEncoding>, BigEndian>,
}

use ndarray::{Array, Array3};

use crate::transcoder::source::video::FramedViewMode;
use crate::utils::cv::is_feature;
use rayon::prelude::IntoParallelIterator;
use serde::Serialize;

impl<
        T: Clone
            + Default
            + FrameValue<Output = T>
            + Copy
            + Serialize
            + Send
            + Sync
            + num_traits::identities::Zero
            + Into<f64>,
    > Framer for FrameSequence<T>
{
    type Output = T;
    fn new(builder: FramerBuilder) -> Self {
        let plane = &builder.plane;

        let chunk_rows = builder.chunk_rows;
        assert!(chunk_rows > 0);

        let num_chunks: usize = ((builder.plane.h()) as f64 / chunk_rows as f64).ceil() as usize;
        let last_chunk_rows = builder.plane.h_usize() - (num_chunks - 1) * chunk_rows;

        assert!(num_chunks > 0);
        let array: Array3<Option<T>> =
            Array3::<Option<T>>::default((chunk_rows, plane.w_usize(), plane.c_usize()));
        let last_array: Array3<Option<T>> =
            Array3::<Option<T>>::default((last_chunk_rows, plane.w_usize(), plane.c_usize()));

        let mut frames = vec![
            VecDeque::from(vec![Frame {
                array,
                filled_count: 0,
            }]);
            num_chunks
        ];

        // Override the last chunk, in case the chunk size doesn't perfectly divide the number of rows
        if let Some(last) = frames.last_mut() {
            *last = VecDeque::from(vec![Frame {
                array: last_array,
                filled_count: 0,
            }]);
        };

        let mut pixel_ts_tracker: Vec<Array3<BigT>> =
            vec![Array3::zeros((chunk_rows, plane.w_usize(), plane.c_usize())); num_chunks];
        if let Some(last) = pixel_ts_tracker.last_mut() {
            *last = Array3::zeros((last_chunk_rows, plane.w_usize(), plane.c_usize()));
        };

        let mut last_frame_intensity_tracker: Vec<Array3<T>> =
            vec![Array3::zeros((chunk_rows, plane.w_usize(), plane.c_usize())); num_chunks];
        if let Some(last) = last_frame_intensity_tracker.last_mut() {
            *last = Array3::zeros((last_chunk_rows, plane.w_usize(), plane.c_usize()));
        };

        let mut last_filled_tracker: Vec<Array3<i64>> =
            vec![Array3::zeros((chunk_rows, plane.w_usize(), plane.c_usize())); num_chunks];
        if let Some(last) = last_filled_tracker.last_mut() {
            *last = Array3::zeros((last_chunk_rows, plane.w_usize(), plane.c_usize()));
        };
        for chunk in &mut last_filled_tracker {
            for mut row in chunk.rows_mut() {
                row.fill(-1);
            }
        }

        let tpf = if let Some(output_fps) = builder.output_fps {
            (builder.tps as f32 / output_fps) as u32
        } else {
            builder.ref_interval
        };

        // Array3::<Option<T>>::new(num_rows, num_cols, num_channels);
        FrameSequence {
            state: FrameSequenceState {
                plane: *plane,
                frames_written: 0,
                view_mode: builder.view_mode,
                tpf,
                tps: builder.tps,
                source: builder.source,
                codec_version: builder.codec_version,
                source_camera: builder.source_camera,
                ref_interval: builder.ref_interval,
                source_dtm: builder.delta_t_max,
                time_mode: builder.time_mode,
            },
            frames,
            frame_idx_offsets: vec![0; num_chunks],
            pixel_ts_tracker,
            last_filled_tracker,
            last_frame_intensity_tracker,
            chunk_filled_tracker: vec![false; num_chunks],
            mode: builder.mode,
            running_intensities: Array::zeros((
                builder.plane.h_usize(),
                builder.plane.w_usize(),
                builder.plane.c_usize(),
            )),
            detect_features: builder.detect_features,
            buffer_limit: builder.buffer_limit,
            features: VecDeque::with_capacity(
                (builder.delta_t_max / builder.ref_interval) as usize,
            ),
            chunk_rows,
            bincode: DefaultOptions::new()
                .with_fixint_encoding()
                .with_big_endian(),
        }
    }

    fn detect_features(&mut self, detect_features: bool) {
        self.detect_features = detect_features;
    }

    ///
    ///
    /// # Examples
    ///
    /// ```
    /// # use adder_codec_core::{Coord, Event, PlaneSize, TimeMode};
    /// # use adder_codec_core::SourceCamera::FramedU8;
    /// # use adder_codec_core::SourceType::U8;
    /// # use adder_codec_rs::framer::driver::FramerMode::INSTANTANEOUS;
    /// # use adder_codec_rs::framer::driver::{FrameSequence, Framer, FramerBuilder};
    ///
    /// let mut frame_sequence: FrameSequence<u8> =
    /// FramerBuilder::new(
    ///             PlaneSize::new(10,10,3).unwrap(), 64)
    ///             .codec_version(1, TimeMode::DeltaT)
    ///             .time_parameters(50000, 1000, 1000, Some(50.0))
    ///             .mode(INSTANTANEOUS)
    ///             .source(U8, FramedU8)
    ///             .finish();
    /// let mut event: Event = Event {
    ///         coord: Coord {
    ///             x: 5,
    ///             y: 5,
    ///             c: Some(1)
    ///         },
    ///         d: 5,
    ///         t: 1000
    ///     };
    /// frame_sequence.ingest_event(&mut event, None);
    /// let elem = frame_sequence.px_at_current(5, 5, 1).unwrap();
    /// assert_eq!(*elem, Some(32));
    /// ```
    fn ingest_event(&mut self, event: &mut Event, last_event: Option<Event>) -> bool {
        let channel = event.coord.c.unwrap_or(0);
        let chunk_num = event.coord.y as usize / self.chunk_rows;

        // Silently handle malformed event
        if chunk_num >= self.frames.len() {
            return false;
        }

        let time = event.t;
        event.coord.y -= (chunk_num * self.chunk_rows) as u16; // Modify the coordinate here, so it gets ingested at the right place

        let frame_chunk = &mut self.frames[chunk_num];
        let last_filled_frame_ref = &mut self.last_filled_tracker[chunk_num]
            [[event.coord.y.into(), event.coord.x.into(), channel.into()]];
        let running_ts_ref = &mut self.pixel_ts_tracker[chunk_num]
            [[event.coord.y.into(), event.coord.x.into(), channel.into()]];
        let frame_idx_offset = &mut self.frame_idx_offsets[chunk_num];
        let last_frame_intensity_ref = &mut self.last_frame_intensity_tracker[chunk_num]
            [[event.coord.y.into(), event.coord.x.into(), channel.into()]];

        let (filled, grew) = ingest_event_for_chunk(
            event,
            frame_chunk,
            running_ts_ref,
            frame_idx_offset,
            last_filled_frame_ref,
            last_frame_intensity_ref,
            &self.state,
            self.buffer_limit,
        );

        self.chunk_filled_tracker[chunk_num] = filled;

        if grew {
            // handle_dtm(
            //     frame_chunk,
            //     &mut self.chunk_filled_tracker[chunk_num],
            //     &mut self.last_filled_tracker[chunk_num],
            //     &mut self.pixel_ts_tracker[chunk_num],
            //     &mut self.last_frame_intensity_tracker[chunk_num],
            //     &self.state,
            // );
        }

        if self.detect_features {
            let last_frame_intensity_ref = &mut self.last_frame_intensity_tracker[chunk_num]
                [[event.coord.y.into(), event.coord.x.into(), channel.into()]];
            // Revert the y coordinate
            event.coord.y += (chunk_num * self.chunk_rows) as u16;
            self.running_intensities
                [[event.coord.y.into(), event.coord.x.into(), channel.into()]] =
                <T as Into<f64>>::into(*last_frame_intensity_ref) as u8;

            if let Some(last) = last_event {
                if time != last.t {
                    // todo!();
                    if is_feature(event.coord, self.state.plane, &self.running_intensities).unwrap()
                    {
                        debug_assert!(self.state.frames_written >= 0);
                        let mut idx = if (time / self.state.tpf) as i64 >= self.state.frames_written
                        {
                            (time / (self.state.tpf) - self.state.frames_written as u32) as usize
                        } else {
                            0
                        };

                        if time % self.state.tpf == 0 && idx > 0 {
                            idx -= 1;
                        }
                        // dbg!(time);
                        // dbg!(self.state.frames_written);
                        // dbg!(idx);
                        if idx >= self.features.len() {
                            if self.features.is_empty() {
                                // Create the first
                                self.features.push_back(FeatureInterval {
                                    end_ts: self.state.tpf as BigT,
                                    features: vec![],
                                });
                                self.features.push_back(FeatureInterval {
                                    end_ts: self.state.tpf as BigT * 2,
                                    features: vec![],
                                });
                            }

                            let new_end_ts = if time % self.state.tpf == 0 {
                                time
                            } else {
                                (time / self.state.tpf + 1) * self.state.tpf
                            } as BigT;

                            let mut running_end_ts =
                                self.features.back().unwrap().end_ts + self.state.tpf as BigT;
                            // dbg!(new_end_ts);
                            // dbg!(running_end_ts);
                            while running_end_ts <= new_end_ts {
                                self.features.push_back(FeatureInterval {
                                    end_ts: running_end_ts,
                                    features: vec![],
                                });
                                running_end_ts += self.state.tpf as BigT;
                            }
                        }

                        // dbg!(self.features.len());
                        // dbg!(self.features[idx].end_ts);
                        if self.features[idx].end_ts < time as BigT {
                            // Allow the player to enable feature detection on the fly
                            self.features[idx].end_ts = time as BigT;
                        }
                        // assert!(self.features[idx].end_ts >= time as BigT);
                        self.features[idx].features.push(event.coord);
                    }
                }
            }
        }

        for chunk in &self.chunk_filled_tracker {
            if !chunk {
                return false;
            }
        }
        debug_assert!(self.is_frame_0_filled());
        true
    }

    fn ingest_events_events(&mut self, mut events: Vec<Vec<Event>>) -> bool {
        // Make sure that the chunk division is aligned between the source and the framer
        assert_eq!(events.len(), self.frames.len());

        (
            &mut events,
            &mut self.frames,
            &mut self.chunk_filled_tracker,
            &mut self.pixel_ts_tracker,
            &mut self.frame_idx_offsets,
            &mut self.last_filled_tracker,
            &mut self.last_frame_intensity_tracker,
        )
            .into_par_iter()
            .for_each(
                |(
                    a,
                    frame_chunk,
                    chunk_filled,
                    chunk_ts_tracker,
                    frame_idx_offset,
                    chunk_last_filled_tracker,
                    last_frame_intensity_tracker,
                )| {
                    for event in a {
                        let channel = event.coord.c.unwrap_or(0);
                        let chunk_num = event.coord.y as usize / self.chunk_rows;
                        event.coord.y -= (chunk_num * self.chunk_rows) as u16; // Modify the coordinate here, so it gets ingested at the right place
                        let last_filled_frame_ref = &mut chunk_last_filled_tracker
                            [[event.coord.y.into(), event.coord.x.into(), channel.into()]];
                        let running_ts_ref = &mut chunk_ts_tracker
                            [[event.coord.y.into(), event.coord.x.into(), channel.into()]];
                        let last_frame_intensity_ref = &mut last_frame_intensity_tracker
                            [[event.coord.y.into(), event.coord.x.into(), channel.into()]];

                        let (filled, _grew) = ingest_event_for_chunk(
                            event,
                            frame_chunk,
                            running_ts_ref,
                            frame_idx_offset,
                            last_filled_frame_ref,
                            last_frame_intensity_ref,
                            &self.state,
                            self.buffer_limit,
                        );
                        *chunk_filled = filled;

                        // if grew {
                        //     handle_dtm(
                        //         frame_chunk,
                        //         chunk_filled,
                        //         chunk_last_filled_tracker,
                        //         chunk_ts_tracker,
                        //         last_frame_intensity_tracker,
                        //         &self.state,
                        //     );
                        // }
                    }
                },
            );

        self.is_frame_0_filled()
    }

    /// For all frames left that we haven't written out yet, for any None pixels, set them to the
    /// last recorded intensity for that pixel.
    ///
    /// Returns `true` if there are frames now ready to write out
    fn flush_frame_buffer(&mut self) -> bool {
        let mut any_nonempty = false;
        // Check if ANY of the frame arrays are nonempty
        for chunk in &self.frames {
            if chunk.len() > 1 {
                any_nonempty = true;
            }
        }
        if any_nonempty {
            for (chunk_num, chunk) in self.frames.iter_mut().enumerate() {
                let frame_chunk = &mut chunk[0];
                // for frame_chunk in chunk.iter_mut() {
                for ((y, x, c), px) in frame_chunk.array.indexed_iter_mut() {
                    if px.is_none() {
                        // If the pixel is empty, set its intensity to the previous intensity we recorded for it
                        *px = Some(self.last_frame_intensity_tracker[chunk_num][[y, x, c]]);

                        // Update the fill tracker
                        frame_chunk.filled_count += 1;

                        // Update the last filled tracker
                        self.last_filled_tracker[chunk_num][[y, x, c]] += 1;

                        // Update the timestamp tracker
                        // chunk_ts_tracker[[y, x, c]] += state.ref_interval as BigT;
                    }
                }

                // Mark the chunk as filled (ready to write out)
                self.chunk_filled_tracker[chunk_num] = true;
            }
        } else {
            eprintln!("marking not filled...");
            self.chunk_filled_tracker[0] = false;
        }

        self.is_frame_0_filled()

        // for chunk in &self.chunk_filled_tracker {
        //     if !chunk {
        //         all_filled = false;
        //     }
        // }
        //
        // all_filled
    }
}

fn handle_dtm<
    T: Clone
        + Default
        + FrameValue<Output = T>
        + Copy
        + Serialize
        + Send
        + Sync
        + num_traits::identities::Zero
        + Into<f64>,
>(
    frame_chunk: &mut VecDeque<Frame<Option<T>>>,
    chunk_filled: &mut bool,
    chunk_last_filled_tracker: &mut Array3<i64>,
    _chunk_ts_tracker: &mut Array3<BigT>,
    last_frame_intensity_tracker: &Array3<T>,
    state: &FrameSequenceState,
) {
    if frame_chunk.len() > ((state.source_dtm / state.ref_interval) + 1) as usize {
        /* Check the last timestamp for the other pixels in this chunk. If they were so long
        ago that dtm time has passed, then we can repeat the last frame's intensity for
        those pixels.
        */
        // Iterate the other pixels in the chunk
        let frame_chunk = &mut frame_chunk[0];

        for ((y, x, c), px) in frame_chunk.array.indexed_iter_mut() {
            if px.is_none() {
                // If the pixel is empty, set its intensity to the previous intensity we recorded for it
                *px = Some(last_frame_intensity_tracker[[y, x, c]]);

                // Update the fill tracker
                frame_chunk.filled_count += 1;

                // Update the last filled tracker
                chunk_last_filled_tracker[[y, x, c]] += 1;

                // Update the timestamp tracker
                // chunk_ts_tracker[[y, x, c]] += state.ref_interval as BigT;
            }
        }

        // Mark the chunk as filled (ready to write out)
        *chunk_filled = true;
    }
}

impl<T: Clone + Default + FrameValue<Output = T> + Serialize> FrameSequence<T> {
    /// Get the number of frames queue'd up to be written
    #[must_use]
    pub fn get_frames_len(&self) -> usize {
        self.frames.len()
    }

    /// Get the number of chunks in a frame
    #[must_use]
    pub fn get_frame_chunks_num(&self) -> usize {
        self.pixel_ts_tracker.len()
    }

    /// Get the reference for the pixel at the given coordinates
    /// # Arguments
    /// * `y` - The y coordinate of the pixel
    /// * `x` - The x coordinate of the pixel
    /// * `c` - The channel of the pixel
    /// # Returns
    /// * `Option<&T>` - The reference to the pixel value
    /// # Errors
    /// * If the frame has not been initialized
    pub fn px_at_current(
        &self,
        y: usize,
        x: usize,
        c: usize,
    ) -> Result<&Option<T>, FrameSequenceError> {
        if self.frames.is_empty() {
            return Err(FrameSequenceError::UninitializedFrame);
        }
        let chunk_num = y / self.chunk_rows;
        let local_row = y - (chunk_num * self.chunk_rows);
        Ok(&self.frames[chunk_num][0].array[[local_row, x, c]])
    }

    /// Get the reference for the pixel at the given coordinates and frame index
    /// # Arguments
    /// * `y` - The y coordinate of the pixel
    /// * `x` - The x coordinate of the pixel
    /// * `c` - The channel of the pixel
    /// * `frame_idx` - The index of the frame to get the pixel from
    /// # Returns
    /// * `Option<&T>` - The reference to the pixel value
    /// # Errors
    /// * If the frame at the given index has not been initialized
    pub fn px_at_frame(
        &self,
        y: usize,
        x: usize,
        c: usize,
        frame_idx: usize,
    ) -> Result<&Option<T>, FrameSequenceError> {
        let chunk_num = y / self.chunk_rows;
        let local_row = y - (chunk_num * self.chunk_rows);
        match self.frames.len() {
            a if frame_idx < a => Ok(&self.frames[chunk_num][frame_idx].array[[local_row, x, c]]),
            _ => Err(FrameSequenceError::InvalidIndex),
        }
    }

    #[allow(clippy::unused_self)]
    fn _get_frame(&self, _frame_idx: usize) -> Result<&Array3<Option<T>>, FrameSequenceError> {
        todo!()
        // match self.frames.len() <= frame_idx {
        //     true => Err(FrameSequenceError::InvalidIndex),
        //     false => Ok(&self.frames[frame_idx].array),
        // }
    }

    /// Get whether or not the frame at the given index is "filled" (i.e., all pixels have been
    /// written to)
    /// # Arguments
    /// * `frame_idx` - The index of the frame to check
    /// # Returns
    /// * `bool` - Whether or not the frame is filled
    /// # Errors
    /// * If the frame at the given index has not been initialized
    /// * If the frame index is out of bounds
    /// * If the frame is not aligned with the chunk division
    pub fn is_frame_filled(&self, frame_idx: usize) -> Result<bool, FrameSequenceError> {
        for chunk in &self.frames {
            if chunk.len() <= frame_idx {
                return Err(FrameSequenceError::InvalidIndex);
            }

            match chunk[frame_idx].filled_count {
                a if a == chunk[0].array.len() => {}
                a if a > chunk[0].array.len() => {
                    return Err(FrameSequenceError::BadFillCount);
                }
                _ => {
                    return Ok(false);
                }
            }
        }
        Ok(true)
    }

    /// Get whether or not the next frame is "filled" (i.e., all pixels have been written to)
    #[must_use]
    pub fn is_frame_0_filled(&self) -> bool {
        if let Some(buffer_limit) = self.buffer_limit {
            for chunk in self.frames.iter() {
                if chunk.len() > buffer_limit as usize {
                    return true;
                }
            }
        }

        for chunk in self.chunk_filled_tracker.iter() {
            if !chunk {
                return false;
            }
        }
        true
    }

    /// Get the instantaneous intensity for each pixel
    pub fn get_running_intensities(&self) -> &Array3<u8> {
        &self.running_intensities
    }

    /// Get the features detected for the next frame, and pop that off the feature vec
    pub fn pop_features(&mut self) -> Option<FeatureInterval> {
        if self.features.is_empty() {
            // Create the first
            self.features.push_back(FeatureInterval {
                end_ts: self.state.tpf as BigT,
                features: vec![],
            });
            // Create the first
            self.features.push_back(FeatureInterval {
                end_ts: self.state.tpf as BigT * 2,
                features: vec![],
            });
        } else {
            self.features.push_back(FeatureInterval {
                end_ts: self.state.tpf as BigT + self.features.back().unwrap().end_ts,
                features: vec![],
            });
        }

        // dbg!("Popping features");
        // dbg!(self.features.front().unwrap().end_ts);
        self.features.pop_front()
    }

    /// Pop the next frame for all chunks
    ///
    /// returns: the frame
    pub fn pop_next_frame(&mut self) -> Option<Vec<Array3<Option<T>>>> {
        let mut ret: Vec<Array3<Option<T>>> = Vec::with_capacity(self.frames.len());

        for chunk_num in 0..self.frames.len() {
            match self.pop_next_frame_for_chunk(chunk_num) {
                Some(frame) => {
                    ret.push(frame);
                }
                None => {
                    println!("Couldn't pop chunk {chunk_num}!");
                }
            }
        }
        self.state.frames_written += 1;
        // dbg!(self.state.frames_written);
        Some(ret)
    }

    /// Pop the next frame from the given chunk
    ///
    /// # Arguments
    ///
    /// * `chunk_num`: the y-index of the chunk to pop the frame from
    ///
    /// returns: the chunk of frame values
    pub fn pop_next_frame_for_chunk(&mut self, chunk_num: usize) -> Option<Array3<Option<T>>> {
        self.frames[chunk_num].rotate_left(1);
        match self.frames[chunk_num].pop_back() {
            Some(a) => {
                // If this is the only frame left, then add a new one to prevent invalid accesses later
                if self.frames[chunk_num].is_empty() {
                    let array: Array3<Option<T>> = Array3::<Option<T>>::default(a.array.raw_dim());
                    self.frames[chunk_num].append(&mut VecDeque::from(vec![
                        Frame {
                            array,
                            filled_count: 0
                        };
                        1
                    ]));
                    self.frame_idx_offsets[chunk_num] += 1;
                }
                self.chunk_filled_tracker[chunk_num] =
                    self.frames[chunk_num][0].filled_count == self.frames[chunk_num][0].array.len();
                Some(a.array)
            }
            None => None,
        }
    }

    /// Write out the next frame to the given writer
    /// # Arguments
    /// * `writer` - The writer to write the frame to
    /// # Returns
    /// * `Result<(), FrameSequenceError>` - Whether or not the write was successful
    /// # Errors
    /// * If the frame chunk has not been initialized
    /// * If the data cannot be written
    pub fn write_frame_bytes(
        &mut self,
        writer: &mut BufWriter<File>,
    ) -> Result<(), Box<dyn Error>> {
        let none_val = T::default();
        for chunk_num in 0..self.frames.len() {
            match self.pop_next_frame_for_chunk(chunk_num) {
                Some(arr) => {
                    for px in arr.iter() {
                        self.bincode.serialize_into(
                            &mut *writer,
                            match px {
                                Some(event) => event,
                                None => &none_val,
                            },
                        )?;
                    }
                }
                None => {
                    return Err(FrameSequenceError::UninitializedFrameChunk.into());
                }
            }
        }
        self.state.frames_written += 1;
        dbg!(self.state.frames_written);
        Ok(())
    }

    /// Write out next frames to the given writer so long as the frame is filled
    /// # Arguments
    /// * `writer` - The writer to write the frames to
    /// # Returns
    /// * `Result<(), FrameSequenceError>` - Whether or not the write was successful
    /// # Errors
    /// * If a frame could not be written
    pub fn write_multi_frame_bytes(
        &mut self,
        writer: &mut BufWriter<File>,
    ) -> Result<i32, Box<dyn Error>> {
        let mut frame_count = 0;
        while self.is_frame_filled(0)? {
            self.write_frame_bytes(writer)?;
            frame_count += 1;
        }
        Ok(frame_count)
    }
}

// TODO: refactor this garbage
fn ingest_event_for_chunk<
    T: Clone + Default + FrameValue<Output = T> + Copy + Serialize + Send + Sync + Into<f64>,
>(
    event: &mut Event,
    frame_chunk: &mut VecDeque<Frame<Option<T>>>,
    running_ts_ref: &mut BigT,
    frame_idx_offset: &mut i64,
    last_filled_frame_ref: &mut i64,
    last_frame_intensity_ref: &mut T,
    state: &FrameSequenceState,
    buffer_limit: Option<u32>,
) -> (bool, bool) {
    let channel = event.coord.c.unwrap_or(0);
    let mut grew = false;

    let prev_last_filled_frame = *last_filled_frame_ref;
    let prev_running_ts = *running_ts_ref;

    if state.codec_version >= 2 && state.time_mode == TimeMode::AbsoluteT {
        if prev_running_ts >= event.t as BigT {
            return (
                frame_chunk[0].filled_count == frame_chunk[0].array.len(),
                false,
            );
        }
        *running_ts_ref = event.t as BigT;
    } else {
        *running_ts_ref += u64::from(event.t);
    }

    if ((running_ts_ref.saturating_sub(1)) as i64 / i64::from(state.tpf)) > *last_filled_frame_ref {
        // Set the frame's value from the event

        if event.d != D_EMPTY {
            // If d == 0xFF, then the event was empty, and we simply repeat the last non-empty
            // event's intensity. Else we reset the intensity here.
            let practical_d_max =
                fast_math::log2_raw(T::max_f32() * (state.source_dtm / state.ref_interval) as f32);
            if state.codec_version >= 2
                && state.time_mode == TimeMode::AbsoluteT
                && state.view_mode != FramedViewMode::SAE
            {
                // event.delta_t -= ((*last_filled_frame_ref + 1) * state.ref_interval as i64) as u32;
                event.t = event.t.saturating_sub(prev_running_ts as u32);
            }

            // TODO: Handle SAE view mode
            *last_frame_intensity_ref = T::get_frame_value(
                event,
                state.source,
                state.ref_interval as f64,
                practical_d_max,
                state.source_dtm,
                state.view_mode,
                Some(SaeTime {
                    running_t: *running_ts_ref as DeltaT,
                    last_fired_t: prev_running_ts as DeltaT,
                }), // TODO
            );
        }

        *last_filled_frame_ref = (running_ts_ref.saturating_sub(1)) as i64 / i64::from(state.tpf);

        // Grow the frames vec if necessary
        match *last_filled_frame_ref - *frame_idx_offset {
            a if a > 0 => {
                let array: Array3<Option<T>> =
                    Array3::<Option<T>>::default(frame_chunk[0].array.raw_dim());
                frame_chunk.append(&mut VecDeque::from(vec![
                    Frame {
                        array,
                        filled_count: 0
                    };
                    a as usize
                ]));
                *frame_idx_offset += a;
                grew = true;
            }
            a if a < 0 => {
                // We can get here if we've forcibly popped a frame before it's ready.
                // Increment pixel ts trackers as normal, but don't actually do anything
                // with the intensities if they correspond to frames that we've already
                // popped.
                //
                // ALSO can arrive here if the source events are not perfectly
                // temporally interleaved. This may be the case for transcoder
                // performance reasons. The only invariant we hold is that a sequence
                // of events for a given (individual) pixel is in the correct order.
                // There is no invariant for the relative order or interleaving
                // of different pixel event sequences.
            }
            _ => {}
        }

        let mut px: &mut Option<T>;
        for i in prev_last_filled_frame..*last_filled_frame_ref {
            if i - state.frames_written + 1 >= 0 {
                px = &mut frame_chunk[(i - state.frames_written + 1) as usize].array
                    [[event.coord.y.into(), event.coord.x.into(), channel.into()]];
                match px {
                    Some(_val) => {}
                    None => {
                        *px = Some(*last_frame_intensity_ref);
                        frame_chunk[(i - state.frames_written + 1) as usize].filled_count += 1;
                    }
                }
            }
        }
    }

    // If framed video source, we can take advantage of scheme that reduces event rate by half
    if state.codec_version >= 1
        // && state.time_mode == TimeMode::DeltaT
        && match state.source_camera {
            SourceCamera::FramedU8
            | SourceCamera::FramedU16
            | SourceCamera::FramedU32
            | SourceCamera::FramedU64
            | SourceCamera::FramedF32
            | SourceCamera::FramedF64 => true,
            SourceCamera::Dvs
            | SourceCamera::DavisU8
            | SourceCamera::Atis
            | SourceCamera::Asint => false,
            // TODO: switch statement on the transcode MODE (frame-perfect or continuous), not just the source
        }
        && *running_ts_ref % u64::from(state.ref_interval) > 0
    {
        *running_ts_ref =
            ((*running_ts_ref / u64::from(state.ref_interval)) + 1) * u64::from(state.ref_interval);
    }

    if let Some(buffer_limit) = buffer_limit {
        // dbg!("buffer filled");
        if *last_filled_frame_ref > state.frames_written + buffer_limit as i64 {
            // dbg!("buffer filled 2");
            frame_chunk[0].filled_count = frame_chunk[0].array.len();
        }
    }

    debug_assert!(*last_filled_frame_ref >= 0);
    if frame_chunk[0].filled_count > frame_chunk[0].array.len() {
        frame_chunk[0].filled_count = frame_chunk[0].array.len();
    }

    (
        frame_chunk[0].filled_count == frame_chunk[0].array.len(),
        grew,
    )
}
