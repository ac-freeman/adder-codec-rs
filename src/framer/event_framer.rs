use crate::framer::scale_intensity::FrameValue;
use crate::{BigT, DeltaT, Event, SourceCamera, D};
use bincode::config::{BigEndian, FixintEncoding, WithOtherEndian, WithOtherIntEncoding};
use bincode::{DefaultOptions, Options};
use rayon::iter::IndexedParallelIterator;
use rayon::iter::ParallelIterator;
use std::cmp::max;
use std::collections::VecDeque;
use std::fs::File;
use std::io::BufWriter;
use std::process::Output;

// Want one main framer with the same functions
// Want additional functions
// Want ability to get instantaneous frames at a fixed interval, or at api-spec'd times
// Want ability to get full integration frames at a fixed interval, or at api-spec'd times

/// An ADΔER event representation
#[derive(Debug, Copy, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct EventCoordless {
    pub d: D,
    pub delta_t: DeltaT,
}

pub enum FramerMode {
    INSTANTANEOUS,
    INTEGRATION,
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub enum SourceType {
    U8,
    U16,
    U32,
    U64,
    F32,
    F64,
}

pub trait Framer {
    type Output;
    fn new(
        num_rows: usize,
        num_cols: usize,
        num_channels: usize,
        tps: DeltaT,
        output_fps: u32,
        mode: FramerMode,
        source: SourceType,
        codec_version: u8,
        source_camera: SourceCamera,
        ref_interval: DeltaT,
    ) -> Self;

    /// Ingest an ADDER event. Will process differently depending on choice of [`FramerMode`].
    ///
    /// If [`INSTANTANEOUS`], this function will set the corresponding output frame's pixel value to
    /// the value derived from this [`Event`], only if this is the first value ingested for that
    /// pixel and frame. Otherwise, the operation will silently be ignored.
    ///
    /// If [`INTEGRATION`], this function will integrate this [`Event`] value for the corresponding
    /// output frame(s)
    fn ingest_event(&mut self, event: &mut Event) -> bool;

    // fn ingest_event_for_chunk(
    //     &self,
    //     event: &Event,
    //     frame_chunk: &mut VecDeque<Frame<Option<Self::Output>>>,
    //     pixel_ts_tracker: &mut BigT,
    //     frame_idx_offset: &mut i64,
    //     last_filled_tracker: &mut i64,
    // ) -> bool;
    fn ingest_events_events(&mut self, events: Vec<Vec<Event>>) -> bool;
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Frame<T> {
    pub(crate) array: Array3<T>,
    pub(crate) filled_count: usize,
}

#[derive(Debug)]
pub enum FrameSequenceError {
    /// Frame index out of bounds
    InvalidIndex,
}

pub struct FrameSequence<T> {
    pub(crate) frames: Vec<VecDeque<Frame<Option<T>>>>,
    pub(crate) frames_written: i64,
    pub(crate) frame_idx_offsets: Vec<i64>,
    pub(crate) pixel_ts_tracker: Vec<Array3<BigT>>,
    pub(crate) last_filled_tracker: Vec<Array3<i64>>,
    chunk_filled_tracker: Vec<bool>,
    pub(crate) mode: FramerMode,
    pub(crate) tpf: DeltaT,
    pub(crate) source: SourceType,
    codec_version: u8,
    source_camera: SourceCamera,
    ref_interval: DeltaT,
    pub chunk_rows: usize,
    bincode: WithOtherEndian<WithOtherIntEncoding<DefaultOptions, FixintEncoding>, BigEndian>,
}

use ndarray::Array3;
use rayon::current_num_threads;
use rayon::iter::IntoParallelRefIterator;
use rayon::prelude::{IntoParallelIterator, IntoParallelRefMutIterator};
use serde::Serialize;

impl<T: Clone + Default + FrameValue<Output = T> + Copy + Serialize + Send + Sync> Framer
    for FrameSequence<T>
{
    type Output = T;
    fn new(
        num_rows: usize,
        num_cols: usize,
        num_channels: usize,
        tps: DeltaT,
        output_fps: u32,
        mode: FramerMode,
        source: SourceType,
        codec_version: u8,
        source_camera: SourceCamera,
        ref_interval: DeltaT,
    ) -> Self {
        let chunk_rows = max(num_rows / current_num_threads(), 1);
        assert!(chunk_rows > 0);

        let num_chunks: usize = ((num_rows) as f64 / chunk_rows as f64).ceil() as usize;
        let last_chunk_rows = num_rows - (num_chunks - 1) * chunk_rows;
        let px_per_chunk: usize = chunk_rows * num_cols * num_channels as usize;

        assert!(num_chunks > 0);
        let array: Array3<Option<T>> =
            Array3::<Option<T>>::default((chunk_rows, num_cols, num_channels));
        let last_array: Array3<Option<T>> =
            Array3::<Option<T>>::default((last_chunk_rows, num_cols, num_channels));

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
            }])
        };

        let mut pixel_ts_tracker: Vec<Array3<BigT>> =
            vec![Array3::zeros((chunk_rows, num_cols, num_channels)); num_chunks];
        if let Some(last) = pixel_ts_tracker.last_mut() {
            *last = Array3::zeros((last_chunk_rows, num_cols, num_channels))
        };

        let mut last_filled_tracker: Vec<Array3<i64>> =
            vec![Array3::zeros((chunk_rows, num_cols, num_channels)); num_chunks];
        if let Some(last) = last_filled_tracker.last_mut() {
            *last = Array3::zeros((last_chunk_rows, num_cols, num_channels))
        };
        for mut chunk in &mut last_filled_tracker {
            for mut row in chunk.rows_mut() {
                row.fill(-1);
            }
        }

        // Array3::<Option<T>>::new(num_rows, num_cols, num_channels);
        FrameSequence {
            frames,
            frames_written: 0,
            frame_idx_offsets: vec![0; num_chunks],
            pixel_ts_tracker,
            last_filled_tracker,
            chunk_filled_tracker: vec![false; num_chunks],
            mode,
            tpf: tps / output_fps,
            source,
            codec_version,
            source_camera,
            ref_interval,
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
    /// # use adder_codec_rs::{Coord, Event};
    /// # use adder_codec_rs::framer::event_framer::FramerMode::INSTANTANEOUS;
    /// # use adder_codec_rs::framer::event_framer::{FrameSequence, Framer};
    /// # use adder_codec_rs::framer::event_framer::SourceType::U8;
    /// use adder_codec_rs::SourceCamera::FramedU8;
    ///
    /// let mut frame_sequence: FrameSequence<u8> = FrameSequence::<u8>::new(10, 10, 3, 50000, 50, INSTANTANEOUS, U8, 1, FramedU8, 1500);
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
    /// let elem = frame_sequence.px_at_current(5, 5, 1);
    /// assert_eq!(*elem, Some(32));
    /// ```
    fn ingest_event(&mut self, event: &mut Event) -> bool {
        let channel = event.coord.c.unwrap_or(0);
        let chunk_num = event.coord.y as usize / self.chunk_rows;
        let tmp = event.clone();
        event.coord.y -= (chunk_num * self.chunk_rows) as u16; // Modify the coordinate here, so it gets ingested at the right place

        let frame_chunk = &mut self.frames[chunk_num];
        let last_filled_frame_ref = &mut self.last_filled_tracker[chunk_num]
            [[event.coord.y.into(), event.coord.x.into(), channel.into()]];
        let running_ts_ref = &mut self.pixel_ts_tracker[chunk_num]
            [[event.coord.y.into(), event.coord.x.into(), channel.into()]];
        let frame_idx_offset = &mut self.frame_idx_offsets[chunk_num];
        self.chunk_filled_tracker[chunk_num] = ingest_event_for_chunk(
            event,
            frame_chunk,
            running_ts_ref,
            frame_idx_offset,
            last_filled_frame_ref,
            self.frames_written,
            self.tpf,
            self.source,
            self.codec_version,
            self.source_camera,
            self.ref_interval,
        );
        for chunk in &self.chunk_filled_tracker {
            if !chunk {
                return false;
            }
        }
        debug_assert!(self.is_frame_0_filled().unwrap());
        true
    }

    fn ingest_events_events(&mut self, mut events: Vec<Vec<Event>>) -> bool {
        (
            &mut events,
            &mut self.frames,
            &mut self.chunk_filled_tracker,
            &mut self.pixel_ts_tracker,
            &mut self.frame_idx_offsets,
            &mut self.last_filled_tracker,
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
                )| {
                    for event in a {
                        let channel = event.coord.c.unwrap_or(0);
                        let chunk_num = event.coord.y as usize / self.chunk_rows;
                        let tmp = event.clone();
                        event.coord.y -= (chunk_num * self.chunk_rows) as u16; // Modify the coordinate here, so it gets ingested at the right place
                        let last_filled_frame_ref = &mut chunk_last_filled_tracker
                            [[event.coord.y.into(), event.coord.x.into(), channel.into()]];
                        let running_ts_ref = &mut chunk_ts_tracker
                            [[event.coord.y.into(), event.coord.x.into(), channel.into()]];

                        *chunk_filled = ingest_event_for_chunk(
                            event,
                            frame_chunk,
                            running_ts_ref,
                            frame_idx_offset,
                            last_filled_frame_ref,
                            self.frames_written,
                            self.tpf,
                            self.source,
                            self.codec_version,
                            self.source_camera,
                            self.ref_interval,
                        );
                    }
                },
            );

        self.is_frame_0_filled().unwrap()
    }
}

impl<T: Clone + Default + FrameValue<Output = T> + Serialize> FrameSequence<T> {
    pub fn px_at_current(&self, row: usize, col: usize, channel: usize) -> &Option<T> {
        if self.frames.is_empty() {
            panic!("Frame not initialized");
        }
        let chunk_num = row / self.chunk_rows;
        let local_row = row - (chunk_num * self.chunk_rows);
        &self.frames[chunk_num][0].array[[local_row, col, channel]]
    }

    pub fn px_at_frame(
        &self,
        row: usize,
        col: usize,
        channel: usize,
        frame_idx: usize,
    ) -> Result<&Option<T>, FrameSequenceError> {
        let chunk_num = row / self.chunk_rows;
        let local_row = row - (chunk_num * self.chunk_rows);
        match self.frames.len() {
            a if frame_idx < a => {
                Ok(&self.frames[chunk_num][frame_idx].array[[local_row, col, channel]])
            }
            _ => {
                Err(FrameSequenceError::InvalidIndex) // TODO: not the right error
            }
        }
    }

    fn _get_frame(&self, frame_idx: usize) -> Result<&Array3<Option<T>>, FrameSequenceError> {
        todo!()
        // match self.frames.len() <= frame_idx {
        //     true => Err(FrameSequenceError::InvalidIndex),
        //     false => Ok(&self.frames[frame_idx].array),
        // }
    }

    pub fn is_frame_filled(&self, frame_idx: usize) -> Result<bool, FrameSequenceError> {
        for chunk in &self.frames {
            match chunk.len() <= frame_idx {
                true => {
                    return Err(FrameSequenceError::InvalidIndex);
                }
                false => match chunk[frame_idx as usize].filled_count {
                    a if a == chunk[0].array.len() => {}
                    a if a > chunk[0].array.len() => {
                        panic!("Impossible fill count. File a bug report!")
                    }
                    _ => {
                        return Ok(false);
                    }
                },
            };
        }
        Ok(true)
    }

    pub fn is_frame_0_filled(&self) -> Result<bool, FrameSequenceError> {
        for chunk in &self.chunk_filled_tracker {
            if !chunk {
                return Ok(false);
            }
        }
        Ok(true)
    }

    pub fn pop_next_frame(&mut self) -> Option<Vec<Array3<Option<T>>>> {
        let mut ret: Vec<Array3<Option<T>>> = Vec::with_capacity(self.frames.len());

        for chunk_num in 0..self.frames.len() {
            match self.pop_next_frame_for_chunk(chunk_num) {
                Some(frame) => {
                    ret.push(frame);
                }
                None => {
                    println!("Couldn't pop chunk {}!", chunk_num)
                }
            }
        }
        self.frames_written += 1;
        Some(ret)
    }

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
                self.chunk_filled_tracker[chunk_num] = false;
                Some(a.array)
            }
            None => panic!("No frame to pop"), // TODO: remove again
        }
    }

    pub fn write_frame_bytes(&mut self, writer: &mut BufWriter<File>) {
        for chunk_num in 0..self.frames.len() {
            match self.pop_next_frame_for_chunk(chunk_num) {
                Some(arr) => {
                    let none_val = T::default();
                    for px in arr.iter() {
                        match self.bincode.serialize_into(
                            &mut *writer,
                            match px {
                                Some(event) => event,
                                None => &none_val,
                            },
                        ) {
                            Ok(_) => {}
                            Err(e) => {
                                panic!("{}", e)
                            }
                        };
                    }
                }
                None => {
                    println!("Couldn't pop chunk {}!", chunk_num)
                }
            }
        }
        self.frames_written += 1;
    }

    pub fn write_multi_frame_bytes(&mut self, writer: &mut BufWriter<File>) -> i32 {
        let mut frame_count = 0;
        while self.is_frame_filled(0).unwrap() {
            self.write_frame_bytes(writer);
            frame_count += 1;
        }
        frame_count
    }
}

fn ingest_event_for_chunk<
    T: Clone + Default + FrameValue<Output = T> + Copy + Serialize + Send + Sync,
>(
    event: &Event,
    frame_chunk: &mut VecDeque<Frame<Option<T>>>,
    running_ts_ref: &mut BigT,
    frame_idx_offset: &mut i64,
    last_filled_frame_ref: &mut i64,
    frames_written: i64,
    tpf: DeltaT,
    source: SourceType,
    codec_version: u8,
    source_camera: SourceCamera,
    ref_interval: DeltaT,
) -> bool {
    let channel = event.coord.c.unwrap_or(0);

    let prev_last_filled_frame = *last_filled_frame_ref;
    let _already_filled = *last_filled_frame_ref >= frames_written;

    *running_ts_ref += event.delta_t as BigT;

    if ((*running_ts_ref - 1) as i64 / tpf as i64) > *last_filled_frame_ref {
        match event.d {
            d if d == 0xFF && event.delta_t < tpf => {
                // Don't do anything -- it's an empty event
                // Except in special case where delta_t == tpf
                if *running_ts_ref == tpf as BigT && event.delta_t == tpf {
                    frame_chunk[(*last_filled_frame_ref - *frame_idx_offset) as usize].array
                        [[event.coord.y.into(), event.coord.x.into(), channel.into()]] =
                        Some(T::default());
                    frame_chunk[(*last_filled_frame_ref - *frame_idx_offset) as usize]
                        .filled_count += 1;
                    // if (*last_filled_frame_ref - self.frame_idx_offset) == 0 {
                    //     println!("{}, {}", event.coord.x, event.coord.y);
                    // }

                    *last_filled_frame_ref = ((*running_ts_ref - 1) as i64 / tpf as i64) + 1;
                }
            }
            _ => {
                let scaled_intensity: T = T::get_frame_value(event, source, tpf);
                *last_filled_frame_ref = (*running_ts_ref - 1) as i64 / tpf as i64;

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
                    if i - frames_written + 1 >= 0 {
                        let tmp = &mut frame_chunk[(i - frames_written + 1) as usize].array;
                        let tmp2 =
                            tmp[[event.coord.y.into(), event.coord.x.into(), channel.into()]];
                        frame = &mut frame_chunk[(i - frames_written + 1) as usize].array
                            [[event.coord.y.into(), event.coord.x.into(), channel.into()]];
                        match frame {
                            Some(_val) => {}
                            None => {
                                *frame = Some(scaled_intensity);
                                frame_chunk[(i - frames_written + 1) as usize].filled_count += 1;
                                // if (i - self.frames_written + 1) == 0 {
                                //     println!("{}, {}", event.coord.x, event.coord.y);
                                // }
                            }
                        }
                    }
                }
            }
        }
    }

    // If framed video source, we can take advantage of scheme that reduces event rate by half
    if codec_version > 0
        && match source_camera {
            SourceCamera::FramedU8 => true,
            SourceCamera::FramedU16 => true,
            SourceCamera::FramedU32 => true,
            SourceCamera::FramedU64 => true,
            SourceCamera::FramedF32 => true,
            SourceCamera::FramedF64 => true,
            SourceCamera::Dvs => false,
            SourceCamera::DavisU8 => false,
            SourceCamera::Atis => false,
            SourceCamera::Asint => false,
        }
        && *running_ts_ref % ref_interval as BigT > 0
    {
        *running_ts_ref = (*last_filled_frame_ref as BigT + 1) * ref_interval as BigT;
    }

    debug_assert!(*last_filled_frame_ref >= 0);
    debug_assert!(frame_chunk[0].filled_count <= frame_chunk[0].array.len());
    frame_chunk[0].filled_count == frame_chunk[0].array.len()
}
