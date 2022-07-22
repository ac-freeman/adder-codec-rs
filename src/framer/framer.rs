use crate::{BigT, D, D_SHIFT, DeltaT, Event, Intensity};
use crate::framer::array3d::{Array3D, Array3DError};
use crate::framer::array3d::Array3DError::InvalidIndex;
use crate::framer::framer::FramerMode::INSTANTANEOUS;

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
           mode: FramerMode) -> Self;

    fn ingest_event(&mut self, event: &Event) -> Result<(), Array3DError>;

    // fn event_to_scaled_intensity(&self, event: &Event) -> Intensity {
    //     let intensity = event_to_intensity(event);
    //     (((D_SHIFT[event.d as usize] as f32) / (u8::MAX as f32))
    //         * (self.ticks_per_frame as f32 / event.delta_t as f32)
    //         * u16::MAX as f32) as u16}
    // }
    // fn get_instant_frame(&mut self) ->
}

struct Frame<T> {
    array: Array3D<T>,
    start_ts: BigT,
}

pub struct FrameSequence<T, S> {
    frames: Vec<Frame<T>>,
    mode: FramerMode,
    running_ts: BigT,
    tps: DeltaT,
    output_fps: u32,
    d_max: D,
    delta_t_max: DeltaT,
    source_0: S,
    dest_0: T
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

impl<T: Default, S: Default> Framer for FrameSequence<EventCoordless, S> {
    fn new(num_rows: usize, num_cols: usize, num_channels: usize, tps: DeltaT, output_fps: u32, d_max: D, delta_t_max: DeltaT, _: FramerMode) -> Self {
        let array: Array3D<EventCoordless> = Array3D::new(num_rows, num_cols, num_channels);
        FrameSequence {
            frames: vec![Frame { array, start_ts: 0 }],
            mode: INSTANTANEOUS,    // Silently ignore the mode that's passed in
            running_ts: 0,
            tps,
            output_fps,
            d_max,
            delta_t_max,
            source_0: S::default(),
            dest_0: EventCoordless::default(),
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
}

use duplicate::duplicate_item;
#[duplicate_item(name; [u8]; [u16]; [u32]; [u64])]
impl<T: Default, S: Default + ScaleIntensity<T>> Framer for FrameSequence<name, S> {
    fn new(num_rows: usize, num_cols: usize, num_channels: usize, tps: DeltaT, output_fps: u32, d_max: D, delta_t_max: DeltaT, mode: FramerMode) -> Self {
        let array: Array3D<name> = Array3D::new(num_rows, num_cols, num_channels);
        FrameSequence {
            frames: vec![Frame { array, start_ts: 0 }],
            mode,    // Silently ignore the mode that's passed in
            running_ts: 0,
            tps,
            output_fps,
            d_max,
            delta_t_max,
            source_0: S::default(),
            dest_0: name::default()
        }
    }


    ///
    ///
    /// # Examples
    ///
    /// ```
    // # use adder_codec_rs::{Coord, Event};
    // # use adder_codec_rs::framer::framer::FramerMode::INSTANTANEOUS;
    // # use adder_codec_rs::framer::framer::{FrameSequence, Framer};
    // // Left parameter is the destination format, right parameter is the source format (before
    // // transcoding to ADDER)
    // let mut frame_sequence: FrameSequence<u16, u8> = Framer::<u16, u8>::new(10, 10, 3, 50000, 10, 15, 50000, INSTANTANEOUS);
    // let event: Event = Event {
    //         coord: Coord {
    //             x: 5,
    //             y: 5,
    //             c: Some(1)
    //         },
    //         d: 5,
    //         delta_t: 1000
    //     };
    // let t = frame_sequence.ingest_event(&event);
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
                <S as ScaleIntensity<T>>::scale_intensity(intensity, (self.tps / self.output_fps) as BigT);


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


fn event_to_intensity(event: &Event) -> Intensity {
    D_SHIFT[event.d as usize] as Intensity / event.delta_t as Intensity
}


trait ScaleIntensity <T> {
    fn scale_intensity(intensity: Intensity, tpf: BigT) -> T;
}

/// Scales the event's intensity for a u8 source to a u16 output
impl ScaleIntensity<u16> for u8 {
    fn scale_intensity(intensity: Intensity, tpf: BigT) -> u16 {
        (intensity / u8::MAX as f64 * tpf as f64 * u16::MAX as f64) as u16
    }
}

/// Scales the event's intensity for a u8 source to a u32 output
impl ScaleIntensity<u32> for u8 {
    fn scale_intensity(intensity: Intensity, tpf: BigT) -> u32 {
        (intensity / u8::MAX as f64 * tpf as f64 * u32::MAX as f64) as u32
    }
}

/// Scales the event's intensity for a u8 source to a u32 output
impl ScaleIntensity<u64> for u8 {
    fn scale_intensity(intensity: Intensity, tpf: BigT) -> u64 {
        (intensity / u8::MAX as f64 * tpf as f64 * u64::MAX as f64) as u64
    }
}

