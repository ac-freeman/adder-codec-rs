use crate::framer::scale_intensity::FrameValue;
use crate::{BigT, DeltaT, Event, D};
use bincode::config::{BigEndian, FixintEncoding, WithOtherEndian, WithOtherIntEncoding};
use bincode::{DefaultOptions, Options};
use std::collections::VecDeque;
use std::fs::File;
use std::io::BufWriter;

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
    ) -> Self;

    /// Ingest an ADDER event. Will process differently depending on choice of [`FramerMode`].
    ///
    /// If [`INSTANTANEOUS`], this function will set the corresponding output frame's pixel value to
    /// the value derived from this [`Event`], only if this is the first value ingested for that
    /// pixel and frame. Otherwise, the operation will silently be ignored.
    ///
    /// If [`INTEGRATION`], this function will integrate this [`Event`] value for the corresponding
    /// output frame(s)
    fn ingest_event(&mut self, event: &Event) -> bool;
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
    pub(crate) frames: VecDeque<Frame<Option<T>>>,
    pub(crate) frames_written: i64,
    pub(crate) frame_idx_offset: i64,
    pub(crate) pixel_ts_tracker: Array3<BigT>,
    pub(crate) last_filled_tracker: Array3<i64>,
    pub(crate) mode: FramerMode,
    pub(crate) tpf: DeltaT,
    pub(crate) source: SourceType,
    bincode: WithOtherEndian<WithOtherIntEncoding<DefaultOptions, FixintEncoding>, BigEndian>,
}

use ndarray::Array3;
use serde::Serialize;

impl<T: Clone + Default + FrameValue<Output = T> + Copy> Framer for FrameSequence<T> {
    type Output = T;
    fn new(
        num_rows: usize,
        num_cols: usize,
        num_channels: usize,
        tps: DeltaT,
        output_fps: u32,
        mode: FramerMode,
        source: SourceType,
    ) -> Self {
        let array: Array3<Option<T>> =
            Array3::<Option<T>>::default((num_rows, num_cols, num_channels));
        let mut last_filled_tracker = Array3::zeros((num_rows, num_cols, num_channels));
        for mut row in last_filled_tracker.rows_mut() {
            row.fill(-1);
        }
        // Array3::<Option<T>>::new(num_rows, num_cols, num_channels);
        FrameSequence {
            frames: VecDeque::from(vec![Frame {
                array,
                filled_count: 0,
            }]),
            frames_written: 0,
            frame_idx_offset: 0,
            pixel_ts_tracker: Array3::zeros((num_rows, num_cols, num_channels)),
            last_filled_tracker,
            mode,
            tpf: tps / output_fps,
            source,
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
    ///
    /// let mut frame_sequence: FrameSequence<u8> = FrameSequence::<u8>::new(10, 10, 3, 50000, 50, INSTANTANEOUS, U8);
    /// let event: Event = Event {
    ///         coord: Coord {
    ///             x: 5,
    ///             y: 5,
    ///             c: Some(1)
    ///         },
    ///         d: 5,
    ///         delta_t: 1000
    ///     };
    /// frame_sequence.ingest_event(&event);
    /// let elem = frame_sequence.px_at_current(5, 5, 1);
    /// assert_eq!(*elem, Some(32));
    /// ```
    fn ingest_event(&mut self, event: &Event) -> bool {
        let channel = match event.coord.c {
            None => 0,
            Some(c) => c,
        };

        let last_filled_frame_ref = &mut self.last_filled_tracker
            [[event.coord.y.into(), event.coord.x.into(), channel.into()]];
        let prev_last_filled_frame = *last_filled_frame_ref;
        let _already_filled = *last_filled_frame_ref >= self.frames_written;
        let running_ts_ref = &mut self.pixel_ts_tracker
            [[event.coord.y.into(), event.coord.x.into(), channel.into()]];
        *running_ts_ref += event.delta_t as BigT;

        if ((*running_ts_ref - 1) as i64 / self.tpf as i64) > *last_filled_frame_ref {
            match event.d {
                d if d == 0xFF && event.delta_t < self.tpf => {
                    // Don't do anything -- it's an empty event
                    // Except in special case where delta_t == tpf
                    if *running_ts_ref == self.tpf as BigT && event.delta_t == self.tpf {
                        self.frames[(*last_filled_frame_ref - self.frame_idx_offset) as usize]
                            .array[[event.coord.y.into(), event.coord.x.into(), channel.into()]] =
                            Some(T::default());
                        self.frames[(*last_filled_frame_ref - self.frame_idx_offset) as usize]
                            .filled_count += 1;
                        // if (*last_filled_frame_ref - self.frame_idx_offset) == 0 {
                        //     println!("{}, {}", event.coord.x, event.coord.y);
                        // }

                        *last_filled_frame_ref =
                            ((*running_ts_ref - 1) as i64 / self.tpf as i64) + 1;
                    }
                }
                _ => {
                    let scaled_intensity: T = T::get_frame_value(event, self.source, self.tpf);
                    *last_filled_frame_ref = (*running_ts_ref - 1) as i64 / self.tpf as i64;

                    // Grow the frames vec if necessary
                    match *last_filled_frame_ref - self.frame_idx_offset {
                        a if a > 0 => {
                            let array: Array3<Option<T>> =
                                Array3::<Option<T>>::default(self.frames[0].array.raw_dim());
                            self.frames.append(&mut VecDeque::from(vec![
                                Frame {
                                    array,
                                    filled_count: 0
                                };
                                a as usize
                            ]));
                            self.frame_idx_offset += a;
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
                        if i - self.frames_written + 1 >= 0 {
                            frame = &mut self.frames[(i - self.frames_written + 1) as usize].array
                                [[event.coord.y.into(), event.coord.x.into(), channel.into()]];
                            match frame {
                                Some(_val) => {}
                                None => {
                                    *frame = Some(scaled_intensity);
                                    self.frames[(i - self.frames_written + 1) as usize]
                                        .filled_count += 1;
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

        debug_assert!(*last_filled_frame_ref >= 0);
        debug_assert!(self.frames[0].filled_count <= self.frames[0].array.len());
        self.frames[0].filled_count == self.frames[0].array.len()
    }
}

impl<T: Clone + Default + FrameValue<Output = T> + Serialize> FrameSequence<T> {
    pub fn px_at_current(&self, row: usize, col: usize, channel: usize) -> &Option<T> {
        if self.frames.is_empty() {
            panic!("Frame not initialized");
        }
        &self.frames[0].array[[row, col, channel]]
    }

    pub fn px_at_frame(
        &self,
        row: usize,
        col: usize,
        channel: usize,
        frame_idx: usize,
    ) -> Result<&Option<T>, FrameSequenceError> {
        match self.frames.len() {
            a if frame_idx < a => Ok(&self.frames[frame_idx].array[[row, col, channel]]),
            _ => {
                Err(FrameSequenceError::InvalidIndex) // TODO: not the right error
            }
        }
    }

    fn _get_frame(&self, frame_idx: usize) -> Result<&Array3<Option<T>>, FrameSequenceError> {
        match self.frames.len() <= frame_idx {
            true => Err(FrameSequenceError::InvalidIndex),
            false => Ok(&self.frames[frame_idx].array),
        }
    }

    pub fn is_frame_filled(&self, frame_idx: usize) -> Result<bool, FrameSequenceError> {
        match self.frames.len() <= frame_idx {
            true => Err(FrameSequenceError::InvalidIndex),
            false => match self.frames[frame_idx as usize].filled_count {
                a if a == self.frames[0].array.len() => Ok(true),
                a if a > self.frames[0].array.len() => {
                    panic!("Impossible fill count. File a bug report!")
                }
                _ => Ok(false),
            },
        }
    }

    fn pop_next_frame(&mut self) -> Option<Array3<Option<T>>> {
        self.frames.rotate_left(1);
        match self.frames.pop_back() {
            Some(a) => {
                self.frames_written += 1;
                // If this is the only frame left, then add a new one to prevent invalid accesses later
                if self.frames.is_empty() {
                    let array: Array3<Option<T>> = Array3::<Option<T>>::default(a.array.raw_dim());
                    self.frames.append(&mut VecDeque::from(vec![
                        Frame {
                            array,
                            filled_count: 0
                        };
                        1
                    ]));
                    self.frame_idx_offset += 1;
                }
                Some(a.array)
            }
            None => None,
        }
    }

    pub fn write_frame_bytes(&mut self, writer: &mut BufWriter<File>) {
        match self.pop_next_frame() {
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
            None => {}
        }
    }

    pub fn write_multi_frame_bytes(&mut self, writer: &mut BufWriter<File>) -> i32 {
        let mut frame_count = 0;
        while self.frames[0].filled_count == self.frames[0].array.len() {
            self.write_frame_bytes(writer);
            frame_count += 1;
        }
        frame_count
    }
}
