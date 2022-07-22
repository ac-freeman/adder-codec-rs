use crate::{BigT, D, DeltaT, Event};
use crate::framer::array3d::{Array3D, Array3DError};

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

trait Framer <T> {
    fn new(num_rows: usize,
           num_cols: usize,
           num_channels: usize,
           tps: DeltaT,
           d_max: D,
           delta_t_max: DeltaT,
           mode: FramerMode) -> Self;

    fn ingest_event(&mut self, event: &Event) -> Result<(), Array3DError>;
    // fn get_instant_frame(&mut self) ->
}

struct Frame<T> {
    array: Array3D<T>,
    mode: FramerMode,
    running_ts: BigT,
    tps: DeltaT,
    d_max: D,
    delta_t_max: DeltaT,
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

impl<T> Framer<T> for Frame<EventCoordless> {
    fn new(num_rows: usize, num_cols: usize, num_channels: usize, tps: DeltaT, d_max: D, delta_t_max: DeltaT, mode: FramerMode) -> Self {
        let array: Array3D<EventCoordless> = Array3D::new(num_rows, num_cols, num_channels);
        Frame {
            array,
            mode,
            running_ts: 0,
            tps,
            d_max,
            delta_t_max,
        }
    }

    fn ingest_event(&mut self, event: &crate::Event) -> Result<(), Array3DError> {
        match self.mode {
            FramerMode::INSTANTANEOUS => {
                let channel = match event.coord.c {
                    None => {0}
                    Some(c) => {c}
                };
                self.array.set_at(
                    EventCoordless { d: event.d, delta_t: event.delta_t },
                    event.coord.y.into(), event.coord.x.into(), channel.into())?;
                Ok(())
            }
            FramerMode::INTEGRATION => {
                let channel = match event.coord.c {
                    None => {0}
                    Some(c) => {c}
                };
                self.array.set_at(
                    EventCoordless { d: event.d, delta_t: event.delta_t },
                    event.coord.y.into(), event.coord.x.into(), channel.into())?;
                Ok(())
            }
        }
    }
}



