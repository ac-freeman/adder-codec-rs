use std::collections::VecDeque;
use bytes::{Bytes, BytesMut};
use crate::{BigT, D, DeltaT, Event};
use crate::framer::array3d::{Array3D, Array3DError};
use crate::framer::array3d::Array3DError::InvalidIndex;
use crate::framer::framer::{EventCoordless, Frame, Framer, FramerMode, FrameSequence, SourceType};
use crate::framer::framer::FramerMode::INSTANTANEOUS;

impl From<&EventCoordless> for Bytes {
    fn from(event: &EventCoordless) -> Self {
        Bytes::from([
            &event.d.to_be_bytes() as &[u8],
            &event.delta_t.to_be_bytes() as &[u8]
        ].concat()
        )
    }
}

impl Framer for FrameSequence<EventCoordless> {
    type Output = Option<EventCoordless>;

    fn new(num_rows: usize, num_cols: usize, num_channels: usize, tps: DeltaT, output_fps: u32, d_max: D, delta_t_max: DeltaT, _: FramerMode, source: SourceType) -> Self {
        let array: Array3D<Option<EventCoordless>> = Array3D::new(num_rows, num_cols, num_channels);
        FrameSequence {
            frames: VecDeque::from(vec![Frame { array, start_ts: 0, filled_count: 0 }]),
            current_frame: 0,
            frames_written: 0,
            pixel_ts_tracker: Array3D::new(num_rows, num_cols, num_channels),
            last_filled_tracker: Array3D::new(num_rows, num_cols, num_channels),
            mode: INSTANTANEOUS,    // Silently ignore the mode that's passed in
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
    /// # use adder_codec_rs::framer::framer::{FrameSequence, Framer, EventCoordless};
    /// # use adder_codec_rs::framer::framer::SourceType::U8;
    ///
    /// let mut frame_sequence: FrameSequence<Option<EventCoordless>> = FrameSequence::<Option<EventCoordless>>::new(10, 10, 3, 50000, 50, 15, 50000, INSTANTANEOUS, U8);
    /// let event: Event = Event {
    ///         coord: Coord {
    ///             x: 5,
    ///             y: 5,
    ///             c: Some(1)
    ///         },
    ///         d: 5,
    ///         delta_t: 5000
    ///     };
    ///
    /// let elem = frame_sequence.px_at_frame(5, 5, 1,5);
    /// assert!(elem.is_none());
    /// frame_sequence.ingest_event(&event);
    /// let elem = frame_sequence.px_at_frame(5, 5, 1,5).unwrap();
    /// assert!(elem.is_some());
    /// //let elem = frame_sequence.px_at_current(5, 5, 1).unwrap();
    /// //assert!(elem.is_some())
    /// ```
    fn ingest_event(&mut self, event: &crate::Event) -> Result<bool, Array3DError> {
        let channel = match event.coord.c {
            None => {0}
            Some(c) => {c}
        };

        // Increment the timestamp tracker
        let tracker = self.pixel_ts_tracker.at_mut(event.coord.y.into(), event.coord.x.into(), channel.into()).ok_or(InvalidIndex)?;
        let old_tracker_ts = *tracker;
        let old_frame_num = old_tracker_ts as i64 / self.tpf as i64;
        *tracker = *tracker + event.delta_t as BigT;

        // Get the event's corresponding frame number
        let frame_num = *tracker as i64 / self.tpf as i64;

        // If frame_num is too big, grow the frame vec by the difference
        match frame_num as i64 - self.frames.len() as i64 - self.current_frame + 1{
            a if a > 0 => {
                let array: Array3D<Option<EventCoordless>> = Array3D::new_like(&self.frames[0].array);
                self.frames.append(&mut VecDeque::from(vec![Frame { array, start_ts: 0, filled_count: 0 }; a as usize]));

            }
            _ => {}
        }

        // TODO: copy event to previous frames if bigger than tpf

        match frame_num - old_frame_num {
            a if a > 0 => {
                for i in 0..a as usize + 1 {
                    self.frames[i + old_frame_num as usize].array.set_at(
                                Some(EventCoordless { d: event.d, delta_t: event.delta_t }),
                                event.coord.y.into(), event.coord.x.into(), channel.into())?;
                    self.frames[i + old_frame_num as usize].filled_count += 1;
                }
            }
            _ => {}
        }

        Ok(self.frames[frame_num as usize].filled_count == self.frames[0].array.num_elems())
    }
}