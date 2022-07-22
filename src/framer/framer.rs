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
    fn new(num_rows: usize,
           num_cols: usize,
           num_channels: usize,
           tps: DeltaT,
           output_fps: u32,
           d_max: D,
           delta_t_max: DeltaT,
           mode: FramerMode,
           source: SourceType) -> Self;

    fn ingest_event(&mut self, event: &Event) -> Result<(), Array3DError>;


    // fn event_to_scaled_intensity(&self, event: &Event) -> Intensity {
    //     let intensity = event_to_intensity(event);
    //     (((D_SHIFT[event.d as usize] as f32) / (u8::MAX as f32))
    //         * (self.ticks_per_frame as f32 / event.delta_t as f32)
    //         * u16::MAX as f32) as u16}
    // }
    // fn get_instant_frame(&mut self) ->
}

pub(crate) struct Frame<T> {
    pub(crate) array: Array3D<T>,
    start_ts: BigT,
}

pub struct FrameSequence<T> {
    pub(crate) frames: Vec<Frame<T>>,
    mode: FramerMode,
    running_ts: BigT,
    tps: DeltaT,
    output_fps: u32,
    d_max: D,
    delta_t_max: DeltaT,
    source: SourceType,
}


// impl <T: std::default::Default + std::clone::Clone> Framer for Frame<T> {
//     fn new(num_rows: usize,
//            num_cols: usize,
//            num_channels: usize,
//            tps: DeltaT,
//            d_max: D,
//            delta_t_max: DeltaT,
//            mode: FramerMode) -> Self {
//         let array: Array3D<T> = Array3D::new(num_rows, num_cols, num_channels);
//         Frame {
//             array,
//             mode,
//             running_ts: 0,
//             tps,
//             d_max,
//             delta_t_max,
//         }
//     }
//
//     fn ingest_event(&mut self, event: &Event) {
//         match self.mode {
//             FramerMode::INSTANTANEOUS => {
//
//             }
//             FramerMode::INTEGRATION => {
//
//             }
//         }
//     }
//     // type Item = Array3D<Event>;
// }

impl Framer for FrameSequence<EventCoordless> {
    fn new(num_rows: usize, num_cols: usize, num_channels: usize, tps: DeltaT, output_fps: u32, d_max: D, delta_t_max: DeltaT, _: FramerMode, source: SourceType) -> Self {
        let array: Array3D<EventCoordless> = Array3D::new(num_rows, num_cols, num_channels);
        FrameSequence {
            frames: vec![Frame { array, start_ts: 0 }],
            mode: INSTANTANEOUS,    // Silently ignore the mode that's passed in
            running_ts: 0,
            tps,
            output_fps,
            d_max,
            delta_t_max,
            source,
        }
    }

    fn ingest_event(&mut self, event: &crate::Event) -> Result<(), Array3DError> {
        let channel = match event.coord.c {
            None => {0}
            Some(c) => {c}
        };


        // If the output is 1 ADDER event per pixel, can only do instantaneous frame samples
        self.frames[0].array.set_at(
            EventCoordless { d: event.d, delta_t: event.delta_t },
            event.coord.y.into(), event.coord.x.into(), channel.into())?;

        Ok(())
    }

    // fn at_current(&self, row: usize, col: usize, channel: usize) -> Option<&T> {
    //     todo!()
    // }
}
use duplicate::duplicate_item;
#[duplicate_item(name; [u8]; [u16])]
impl Framer for FrameSequence<name>
    {
    fn new(num_rows: usize, num_cols: usize, num_channels: usize, tps: DeltaT, output_fps: u32, d_max: D, delta_t_max: DeltaT, mode: FramerMode, source: SourceType) -> Self {
        let array: Array3D<name> = Array3D::new(num_rows, num_cols, num_channels);
        FrameSequence {
            frames: vec![Frame { array, start_ts: 0 }],
            mode,    // Silently ignore the mode that's passed in
            running_ts: 0,
            tps,
            output_fps,
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
    /// let elem = frame_sequence.at_current(5, 5, 1).unwrap();
    /// assert_eq!(*elem, 32);
    /// ```
    fn ingest_event(&mut self, event: &crate::Event) -> Result<(), Array3DError> {
        let channel = match event.coord.c {
            None => {0}
            Some(c) => {c}
        };

        match &self.mode {
            FramerMode::INSTANTANEOUS => {
                // Event's intensity representation
                let intensity = event_to_intensity(event);
                // <S as ScaleIntensity<T>>::scale_intensity(intensity, (self.tps / self.output_fps) as BigT);
                let scaled_intensity: name = match self.source {
                    SourceType::U8 => {<u8 as ScaleIntensity<name>>::scale_intensity(intensity, (self.tps / self.output_fps) as BigT)},
                    SourceType::U16 => {<u16 as ScaleIntensity<name>>::scale_intensity(intensity, (self.tps / self.output_fps) as BigT)},
                    SourceType::U32 => {<u32 as ScaleIntensity<name>>::scale_intensity(intensity, (self.tps / self.output_fps) as BigT)},
                    SourceType::U64 => {<u64 as ScaleIntensity<name>>::scale_intensity(intensity, (self.tps / self.output_fps) as BigT)},
                    // SourceType::F32 => {<u8 as ScaleIntensity<T>>::scale_intensity(intensity, (self.tps / self.output_fps) as BigT}
                    // SourceType::F64 => {<u8 as ScaleIntensity<T>>::scale_intensity(intensity, (self.tps / self.output_fps) as BigT}
                    _ => {panic!("jkl")}
                };

                // Since we're only looking at the most recent event for each pixel, never need more than one frame
                self.frames[0].array.set_at(scaled_intensity, event.coord.y.into(), event.coord.x.into(), channel.into())?;


            }
            FramerMode::INTEGRATION => {

                // TODO: figure out what the index will be
                let current_integration = self.frames[0].array.at_mut(
                    event.coord.y.into(),
                    event.coord.x.into(),
                    channel.into())
                    .ok_or(InvalidIndex)?;

                current_integration.saturating_add(event_to_intensity(event) as name);    // TODO: check this

            }
        }
        Ok(())
    }

}

#[duplicate_item(name; [u8]; [u16])]
impl FrameSequence<name> {
    pub fn at_current(&self, row: usize, col: usize, channel: usize) -> Option<&name> {
        self.frames[0].array.at(row, col, channel)
    }
}


fn event_to_intensity(event: &Event) -> Intensity {
    D_SHIFT[event.d as usize] as Intensity / event.delta_t as Intensity
}
