use crate::framer::scale_intensity::FrameValue;
use bincode::config::{BigEndian, FixintEncoding, WithOtherEndian, WithOtherIntEncoding};
use bincode::{DefaultOptions, Options};
use rayon::iter::ParallelIterator;

use std::collections::VecDeque;
use std::error::Error;
use std::fmt;

use adder_codec_core::{
    BigT, DeltaT, Event, PlaneSize, SourceCamera, SourceType, TimeMode, D_EMPTY,
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
pub struct FramerBuilder {
    plane: PlaneSize,
    tps: DeltaT,
    output_fps: f64,
    mode: FramerMode,
    view_mode: FramedViewMode,
    source: SourceType,
    codec_version: u8,
    source_camera: SourceCamera,
    time_mode: TimeMode,
    ref_interval: DeltaT,
    delta_t_max: DeltaT,

    /// The number of rows to process in each chunk (thread).
    pub chunk_rows: usize,
}

impl FramerBuilder {
    /// Create a new FramerBuilder.
    #[must_use]
    pub fn new(plane: PlaneSize, chunk_rows: usize) -> FramerBuilder {
        FramerBuilder {
            plane,
            chunk_rows,
            tps: 150_000,
            output_fps: 30.0,
            mode: FramerMode::INSTANTANEOUS,
            view_mode: FramedViewMode::Intensity,
            source: SourceType::U8,
            codec_version: 3,
            source_camera: SourceCamera::default(),
            time_mode: TimeMode::default(),
            ref_interval: 5000,
            delta_t_max: 5000,
        }
    }

    /// Set the time parameters.
    #[must_use]
    pub fn time_parameters(
        mut self,
        tps: DeltaT,
        ref_interval: DeltaT,
        delta_t_max: DeltaT,
        output_fps: f64,
    ) -> FramerBuilder {
        self.tps = tps;
        self.ref_interval = ref_interval;
        self.delta_t_max = delta_t_max;
        self.output_fps = output_fps;
        self
    }

    /// Set the framer mode.
    #[must_use]
    pub fn mode(mut self, mode: FramerMode) -> FramerBuilder {
        self.mode = mode;
        self
    }

    /// Set the view mode.
    #[must_use]
    pub fn view_mode(mut self, mode: FramedViewMode) -> FramerBuilder {
        self.view_mode = mode;
        self
    }

    /// Set the source type and camera.
    #[must_use]
    pub fn source(mut self, source: SourceType, source_camera: SourceCamera) -> FramerBuilder {
        self.source_camera = source_camera;
        self.source = source;
        self
    }

    /// Set the codec_old version and time mode.
    #[must_use]
    pub fn codec_version(mut self, codec_version: u8, time_mode: TimeMode) -> FramerBuilder {
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
            + num_traits::Zero,
    {
        FrameSequence::<T>::new(self)
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
    fn ingest_event(&mut self, event: &mut Event) -> bool;

    /// Ingest a vector of a vector of ADΔER events.
    fn ingest_events_events(&mut self, events: Vec<Vec<Event>>) -> bool;
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
    pub frames_written: i64,

    /// Ticks per output frame
    pub tpf: DeltaT,
    pub(crate) source: SourceType,
    codec_version: u8,
    source_camera: SourceCamera,
    ref_interval: DeltaT,
    source_dtm: DeltaT,
    view_mode: FramedViewMode,
    time_mode: TimeMode,
}

/// A sequence of frames, each of which is a 3D array of [`FrameValue`]s
#[allow(dead_code)]
pub struct FrameSequence<T> {
    /// The state of the frame sequence
    pub state: FrameSequenceState,
    pub(crate) frames: Vec<VecDeque<Frame<Option<T>>>>,
    pub frame_idx_offsets: Vec<i64>,
    pub(crate) pixel_ts_tracker: Vec<Array3<BigT>>,
    pub(crate) last_filled_tracker: Vec<Array3<i64>>,
    pub(crate) last_frame_intensity_tracker: Vec<Array3<T>>,
    chunk_filled_tracker: Vec<bool>,
    pub(crate) mode: FramerMode,

    /// Number of rows per chunk (per thread)
    pub chunk_rows: usize,
    bincode: WithOtherEndian<WithOtherIntEncoding<DefaultOptions, FixintEncoding>, BigEndian>,
}

use ndarray::Array3;

use crate::transcoder::source::video::FramedViewMode;
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
            + num_traits::identities::Zero,
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

        // Array3::<Option<T>>::new(num_rows, num_cols, num_channels);
        FrameSequence {
            state: FrameSequenceState {
                frames_written: 0,
                view_mode: builder.view_mode,
                tpf: builder.tps / builder.output_fps as u32,
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
            chunk_rows,
            bincode: DefaultOptions::new()
                .with_fixint_encoding()
                .with_big_endian(),
        }
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
    ///             .time_parameters(50000, 1000, 1000, 50.0)
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
    ///         delta_t: 1000
    ///     };
    /// frame_sequence.ingest_event(&mut event);
    /// let elem = frame_sequence.px_at_current(5, 5, 1).unwrap();
    /// assert_eq!(*elem, Some(32));
    /// ```
    fn ingest_event(&mut self, event: &mut Event) -> bool {
        let channel = event.coord.c.unwrap_or(0);
        let chunk_num = event.coord.y as usize / self.chunk_rows;

        event.coord.y -= (chunk_num * self.chunk_rows) as u16; // Modify the coordinate here, so it gets ingested at the right place

        let frame_chunk = &mut self.frames[chunk_num];
        let last_filled_frame_ref = &mut self.last_filled_tracker[chunk_num]
            [[event.coord.y.into(), event.coord.x.into(), channel.into()]];
        let running_ts_ref = &mut self.pixel_ts_tracker[chunk_num]
            [[event.coord.y.into(), event.coord.x.into(), channel.into()]];
        let frame_idx_offset = &mut self.frame_idx_offsets[chunk_num];
        let last_frame_intensity_ref = &mut self.last_frame_intensity_tracker[chunk_num]
            [[event.coord.y.into(), event.coord.x.into(), channel.into()]];

        self.chunk_filled_tracker[chunk_num] = ingest_event_for_chunk(
            event,
            frame_chunk,
            running_ts_ref,
            frame_idx_offset,
            last_filled_frame_ref,
            last_frame_intensity_ref,
            &self.state,
        );
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

                        *chunk_filled = ingest_event_for_chunk(
                            event,
                            frame_chunk,
                            running_ts_ref,
                            frame_idx_offset,
                            last_filled_frame_ref,
                            last_frame_intensity_ref,
                            &self.state,
                        );
                    }
                },
            );

        self.is_frame_0_filled()
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
        for chunk in self.chunk_filled_tracker.iter() {
            if !chunk {
                return false;
            }
        }
        true
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
    T: Clone + Default + FrameValue<Output = T> + Copy + Serialize + Send + Sync,
>(
    event: &mut Event,
    frame_chunk: &mut VecDeque<Frame<Option<T>>>,
    running_ts_ref: &mut BigT,
    frame_idx_offset: &mut i64,
    last_filled_frame_ref: &mut i64,
    last_frame_intensity_ref: &mut T,
    state: &FrameSequenceState,
) -> bool {
    let channel = event.coord.c.unwrap_or(0);

    let prev_last_filled_frame = *last_filled_frame_ref;
    let prev_running_ts = *running_ts_ref;

    if state.codec_version >= 2 && state.time_mode == TimeMode::AbsoluteT {
        *running_ts_ref = event.delta_t as BigT;
    } else {
        *running_ts_ref += u64::from(event.delta_t);
    }

    if ((running_ts_ref.saturating_sub(1)) as i64 / i64::from(state.tpf)) > *last_filled_frame_ref {
        // Set the frame's value from the event

        if event.d != D_EMPTY {
            // If d == 0xFF, then the event was empty, and we simply repeat the last non-empty
            // event's intensity. Else we reset the intensity here.
            let practical_d_max =
                fast_math::log2_raw(T::max_f32() * (state.source_dtm / state.ref_interval) as f32);
            if state.codec_version >= 2 && state.time_mode == TimeMode::AbsoluteT {
                // event.delta_t -= ((*last_filled_frame_ref + 1) * state.ref_interval as i64) as u32;
                event.delta_t = event.delta_t.saturating_sub(prev_running_ts as u32);
            }

            *last_frame_intensity_ref = T::get_frame_value(
                event,
                state.source,
                state.ref_interval,
                practical_d_max,
                state.source_dtm,
                state.view_mode,
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

        let mut frame: &mut Option<T>;
        for i in prev_last_filled_frame..*last_filled_frame_ref {
            if i - state.frames_written + 1 >= 0 {
                frame = &mut frame_chunk[(i - state.frames_written + 1) as usize].array
                    [[event.coord.y.into(), event.coord.x.into(), channel.into()]];
                match frame {
                    Some(_val) => {}
                    None => {
                        *frame = Some(*last_frame_intensity_ref);
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

    debug_assert!(*last_filled_frame_ref >= 0);
    debug_assert!(frame_chunk[0].filled_count <= frame_chunk[0].array.len());
    frame_chunk[0].filled_count == frame_chunk[0].array.len()
}
