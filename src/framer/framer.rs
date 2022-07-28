use std::collections::VecDeque;
use std::fs::File;
use std::io::{BufWriter, Error, Write};
use std::mem::size_of;
use bytes::{BufMut, BytesMut};
use crate::{BigT, D, D_SHIFT, DeltaT, Event, Intensity};
use crate::framer::array3d::{Array3D, Array3DError};
use crate::framer::array3d::Array3DError::InvalidIndex;
use crate::framer::framer::FramerMode::INSTANTANEOUS;
use crate::framer::scale_intensity::ScaleIntensity;

// type EventFrame = Array3D<Event>;
// type Intensity8Frame = Array3D<u8>;

// Want one main framer with the same functions
// Want additional functions
// Want ability to get instantaneous frames at a fixed interval, or at api-spec'd times
// Want ability to get full integration frames at a fixed interval, or at api-spec'd times

/// An ADΔER event representation
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct EventCoordless {
    pub d: D,
    pub delta_t: DeltaT,
}

pub enum FramerMode {
    INSTANTANEOUS,
    INTEGRATION,
}

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
    fn new(num_rows: usize,
           num_cols: usize,
           num_channels: usize,
           tps: DeltaT,
           output_fps: u32,
           d_max: D,
           delta_t_max: DeltaT,
           mode: FramerMode,
           source: SourceType) -> Self;

    /// Ingest an ADDER event. Will process differently depending on choice of [`FramerMode`].
    ///
    /// If [`INSTANTANEOUS`], this function will set the corresponding output frame's pixel value to
    /// the value derived from this [`Event`], only if this is the first value ingested for that
    /// pixel and frame. Otherwise, the operation will silently be ignored.
    ///
    /// If [`INTEGRATION`], this function will integrate this [`Event`] value for the corresponding
    /// output frame(s)
    fn ingest_event(&mut self, event: &Event) -> Result<bool, Array3DError>;

    // fn get_frame_bytes(&mut self) -> Option<BytesMut>;

    // fn pop_next_frame(&mut self) -> Result<Array3D<Self::Output>, Array3DError>;
    //
    // fn write_next_frame(&mut self) -> Result<(), Error>;

    // fn get_frame(&self, frame_idx: usize) -> &Array3D<Self::Output>;


    // fn event_to_scaled_intensity(&self, event: &Event) -> Intensity {
    //     let intensity = event_to_intensity(event);
    //     (((D_SHIFT[event.d as usize] as f32) / (u8::MAX as f32))
    //         * (self.ticks_per_frame as f32 / event.delta_t as f32)
    //         * u16::MAX as f32) as u16}
    // }
    // fn get_instant_frame(&mut self) ->
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Frame<T> {
    pub(crate) array: Array3D<T>,
    pub(crate) start_ts: BigT,  // TODO: using this for anything??
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
    pub(crate) pixel_ts_tracker: Array3D<BigT>,
    pub(crate) last_filled_tracker: Array3D<i64>,
    pub(crate) mode: FramerMode,
    pub(crate) running_ts: BigT,
    pub(crate) tps: DeltaT,
    pub(crate) output_fps: u32,
    pub(crate) tpf: DeltaT,
    pub(crate) d_max: D,
    pub(crate) delta_t_max: DeltaT,
    pub(crate) source: SourceType,
}

use duplicate::duplicate_item;
#[duplicate_item(name; [u8];)]
impl Framer for FrameSequence<name>
{
    type Output = name;
    fn new(num_rows: usize, num_cols: usize, num_channels: usize, tps: DeltaT, output_fps: u32, d_max: D, delta_t_max: DeltaT, mode: FramerMode, source: SourceType) -> Self {
        let array: Array3D<Option<name>> = Array3D::new(num_rows, num_cols, num_channels);
        FrameSequence {
            frames: VecDeque::from(vec![Frame { array, start_ts: 0, filled_count: 0 }]),
            frames_written: 0,
            frame_idx_offset: 0,
            pixel_ts_tracker: Array3D::new(num_rows, num_cols, num_channels),
            last_filled_tracker: Array3D::new_init(-1, num_rows, num_cols, num_channels),
            mode,
            running_ts: 0,
            tps,
            output_fps,
            tpf: tps / output_fps,
            d_max,
            delta_t_max,
            source,
        }
    }


    ///
    ///
    /// # Examples
    ///
    /// ```
    /// # use adder_codec_rs::{Coord, Event};
    /// # use adder_codec_rs::framer::framer::FramerMode::INSTANTANEOUS;
    /// # use adder_codec_rs::framer::framer::{FrameSequence, Framer};
    /// # use adder_codec_rs::framer::framer::SourceType::U8;
    ///
    /// let mut frame_sequence: FrameSequence<u8> = FrameSequence::<u8>::new(10, 10, 3, 50000, 50, 15, 50000, INSTANTANEOUS, U8);
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
    /// let elem = frame_sequence.px_at_current(5, 5, 1).unwrap();
    /// assert_eq!(*elem, 32);
    /// ```
    fn ingest_event(&mut self, event: &crate::Event) -> Result<bool, Array3DError> {
        let channel = match event.coord.c {
            None => {0}
            Some(c) => {c}
        };

        let last_filled_frame_ref = self.last_filled_tracker.at_mut(event.coord.y.into(), event.coord.x.into(), channel.into()).ok_or(InvalidIndex)?;
        let prev_last_filled_frame = *last_filled_frame_ref;
        let already_filled = *last_filled_frame_ref >= self.frames_written;
        let running_ts_ref = self.pixel_ts_tracker.at_mut(event.coord.y.into(), event.coord.x.into(), channel.into()).ok_or(InvalidIndex)?;
        *running_ts_ref = *running_ts_ref + event.delta_t as BigT;

        if ((*running_ts_ref - 1) as i64/ self.tpf as i64) > *last_filled_frame_ref {
            match event.d {
                0xFF => {
                    // Don't do anything -- it's an empty event
                    // Except in special case where delta_t == tpf
                    if *running_ts_ref == self.tpf as BigT && event.delta_t == self.tpf {
                        self.frames[(*last_filled_frame_ref - self.frame_idx_offset) as usize].array.set_at(
                            Some(0),
                            event.coord.y.into(), event.coord.x.into(), channel.into())?;
                        self.frames[(*last_filled_frame_ref - self.frame_idx_offset) as usize].filled_count += 1;
                        *last_filled_frame_ref = ((*running_ts_ref -1) as i64 / self.tpf as i64) + 1;
                    }
                }
                _ => {
                    let intensity = event_to_intensity(event);
                    let scaled_intensity: name = match self.source {
                                SourceType::U8 => { <u8 as ScaleIntensity<name>>::scale_intensity(intensity, (self.tps / self.output_fps) as BigT) },
                                SourceType::U16 => { <u16 as ScaleIntensity<name>>::scale_intensity(intensity, (self.tps / self.output_fps) as BigT) },
                                SourceType::U32 => { <u32 as ScaleIntensity<name>>::scale_intensity(intensity, (self.tps / self.output_fps) as BigT) },
                                SourceType::U64 => { <u64 as ScaleIntensity<name>>::scale_intensity(intensity, (self.tps / self.output_fps) as BigT) },
                                // SourceType::F32 => {<u8 as ScaleIntensity<T>>::scale_intensity(intensity, (self.tps / self.output_fps) as BigT}
                                // SourceType::F64 => {<u8 as ScaleIntensity<T>>::scale_intensity(intensity, (self.tps / self.output_fps) as BigT}
                                _ => { panic!("todo") }
                            };
                    *last_filled_frame_ref = ((*running_ts_ref -1) as i64 / self.tpf as i64);


                    // Grow the frames vec if necessary
                    match *last_filled_frame_ref - self.frame_idx_offset {
                        a if a > 0 => {
                            let array = Array3D::new_like(&self.frames[0].array);
                            self.frames.append(&mut VecDeque::from(vec![Frame { array, start_ts: 0, filled_count: 0 }; a as usize]));
                            self.frame_idx_offset += a;
                        }
                        _ => {}
                    }

                    for i in prev_last_filled_frame..*last_filled_frame_ref {
                        match self.frames[(i - self.frames_written + 1) as usize].array.at(
                            event.coord.y.into(), event.coord.x.into(), channel.into()) {
                            Ok(elem) => {
                                match elem {
                                    Some(val) => {

                                    }
                                    None => {
                                        // println!("Making None to Some at {} and frame index {}", event.coord.x, (i - self.frames_written + 1));

                                        self.frames[(i - self.frames_written + 1) as usize].array.set_at(
                                            Some(scaled_intensity),
                                            event.coord.y.into(), event.coord.x.into(), channel.into())?;
                                        self.frames[(i - self.frames_written + 1) as usize].filled_count += 1;
                                    }
                                }
                            }
                            Err(e) => { panic!("todo")}
                        }


                    }

                }
            }

        }

        if event.coord.y == 0 && event.coord.x == 6 {
            assert!(self.frames[0].array.at(
                                            event.coord.y.into(), event.coord.x.into(), channel.into()).unwrap().is_some())
        }

        // if !already_filled && *last_filled_frame_ref >= self.current_frame {
        //     self.frames[self.current_frame as usize - 1 - self.frames_written as usize].filled_count += 1;
        // }
        debug_assert!(*last_filled_frame_ref >= 0);
        debug_assert!(self.frames[0 as usize].filled_count <= self.frames[0].array.num_elems());
        // println!("{}", self.frames[0 as usize].filled_count);//}
        // if self.frames[0 as usize].filled_count >=0 {
        //     let mut i = 0;
        //     for chunk in &self.frames[0].array.iter_2d() {
        //         for elem in chunk {
        //             match *elem {
        //                 None => {
        //                     eprintln!("None at {}", i );
        //                 }
        //                 _ => {}
        //             }
        //             i += 1;
        //         }
        //     }
        // }
        Ok(self.frames[0 as usize].filled_count == self.frames[0].array.num_elems())














        // // Get the event's corresponding frame number
        // let frame_num = (*runnings_ts as i64  -1) / self.tpf as i64 + 1;
        // let already_filled = old_frame_num > self.current_frame;
        //
        // // If frame_num is too big, grow the frame vec by the difference
        // match frame_num as i64 - self.frames.len() as i64 - self.current_frame {
        //     a if a > 0 => {
        //         let array = Array3D::new_like(&self.frames[0].array);
        //         self.frames.append(&mut VecDeque::from(vec![Frame { array, start_ts: 0, filled_count: 0 }; a as usize]));
        //
        //     }
        //     _ => {}
        // }
        //
        // // This happens when we pop a frame before all the pixels are filled
        // if frame_num < self.current_frame {
        //     return Ok(self.frames[0 as usize].filled_count == self.frames[0].array.num_elems())
        // }
        //
        // if event.d != 0xFF {
        //     // Event's intensity representation
        //     let intensity = event_to_intensity(event);
        //     // <S as ScaleIntensity<T>>::scale_intensity(intensity, (self.tps / self.output_fps) as BigT);
        //     let scaled_intensity: name = match self.source {
        //         SourceType::U8 => { <u8 as ScaleIntensity<name>>::scale_intensity(intensity, (self.tps / self.output_fps) as BigT) },
        //         SourceType::U16 => { <u16 as ScaleIntensity<name>>::scale_intensity(intensity, (self.tps / self.output_fps) as BigT) },
        //         SourceType::U32 => { <u32 as ScaleIntensity<name>>::scale_intensity(intensity, (self.tps / self.output_fps) as BigT) },
        //         SourceType::U64 => { <u64 as ScaleIntensity<name>>::scale_intensity(intensity, (self.tps / self.output_fps) as BigT) },
        //         // SourceType::F32 => {<u8 as ScaleIntensity<T>>::scale_intensity(intensity, (self.tps / self.output_fps) as BigT}
        //         // SourceType::F64 => {<u8 as ScaleIntensity<T>>::scale_intensity(intensity, (self.tps / self.output_fps) as BigT}
        //         _ => { panic!("todo") }
        //     };
        //
        //     match frame_num - self.current_frame {
        //         a if a > 0 => {
        //             for i in 0..a as usize + 1 {
        //                 self.frames[i + old_frame_num as usize].array.set_at(
        //                     scaled_intensity,
        //                     event.coord.y.into(), event.coord.x.into(), channel.into())?;
        //             }
        //         }
        //         _ => {
        //             let current_value: name =  *self.frames[old_frame_num as usize].array.at(
        //                 event.coord.y.into(), event.coord.x.into(), channel.into()).unwrap();
        //             if !already_filled { // TODO: not a good way of doing this
        //                 self.frames[old_frame_num as usize].array.set_at(
        //                     scaled_intensity,
        //                     event.coord.y.into(), event.coord.x.into(), channel.into())?;
        //             }
        //         }
        //     }
        //     }
        // }
        //
        // if !already_filled && frame_num >= self.current_frame {
        //     self.frames[0 as usize].filled_count += 1;
        // }

        // if self.frames[0 as usize].filled_count > 510000 {
        // debug_assert!(self.frames[0 as usize].filled_count <= self.frames[0].array.num_elems());
        // println!("{}", self.frames[0 as usize].filled_count);//}
        // Ok(self.frames[0 as usize].filled_count == self.frames[0].array.num_elems())
    }
}

#[duplicate_item(name; [u8];)]
impl FrameSequence<name> {
    pub fn px_at_current(&self, row: usize, col: usize, channel: usize) -> Result<&Option<name>, Array3DError> {
        if self.frames.len() == 0 {
            panic!("Frame not initialized");
        }
        self.frames[0].array.at(row, col, channel)
    }

    pub fn px_at_frame(&self, row: usize, col: usize, channel: usize, frame_idx: usize) -> Result<&Option<name>, Array3DError> {
        match self.frames.len() {
            a if frame_idx < a => {
                self.frames[frame_idx].array.at(row, col, channel)
            }
            _ => {
                Err(Array3DError::InvalidIndex) // TODO: not the right error
            }
        }
    }

    fn get_frame(&self, frame_idx: usize) -> Result<&Array3D<Option<name>>, FrameSequenceError> {
        match self.frames.len() <= frame_idx {
            true => {
                Err(FrameSequenceError::InvalidIndex)
            }
            false => {
                Ok(&self.frames[frame_idx].array)
            }
        }

    }

    pub fn is_frame_filled(&self, frame_idx: usize) -> Result<bool, FrameSequenceError> {
        match self.frames.len() <= frame_idx {
            true => {
                Err(FrameSequenceError::InvalidIndex)
            }
            false => {
                match self.frames[frame_idx as usize].filled_count {
                    a if a == self.frames[0].array.num_elems() => { Ok(true) },
                    a if a > self.frames[0].array.num_elems() => {
                        panic!("Impossible fill count. File a bug report!")
                    },
                    _ => { Ok(false) }
                }
            }
        }
    }

    pub fn pop_next_frame(&mut self) -> Option<Array3D<Option<name>>> {
        self.frames.rotate_left(1);
        match self.frames.pop_back() {
            Some(a) => {
                self.frames_written += 1;
                // self.frame_idx_offset += 1;
                // If this is the only frame left, then add a new one to prevent invalid accesses later
                if self.frames.len() == 0 {
                    let array = Array3D::new_like(&a.array);
                    self.frames.append(&mut VecDeque::from(vec![Frame { array, start_ts: 0, filled_count: 0 }; 1]));
                }
                Some(a.array)
            }
            None => { None }
        }
    }

    pub fn get_frame_bytes(&mut self) -> Option<BytesMut> {
        match self.pop_next_frame() {
            Some(arr) => {
                Some(arr.serialize_to_be_bytes())
            }
            None => {
                None
            }
        }
    }

    pub fn get_multi_frame_bytes(&mut self) -> Option<(i32, BytesMut)> {
        let mut return_buf = BytesMut::with_capacity(self.frames[0].array.num_elems() * size_of::<name>());
        let mut frame_count = 0;
        while self.frames[0].filled_count == self.frames[0].array.num_elems() {
            match self.get_frame_bytes() {
                None => {panic!("TODO: Frame bytes error")}
                Some(bytes) => {
                    frame_count += 1;
                    return_buf.put(bytes)
                }
            }
        }
        match return_buf.len() {
            0 => {None},
            _ => { Some((frame_count, return_buf))}
        }
    }
}


fn event_to_intensity(event: &Event) -> Intensity {
    D_SHIFT[event.d as usize] as Intensity / event.delta_t as Intensity
}
