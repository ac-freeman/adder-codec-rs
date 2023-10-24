use crate::codec::compressed::source_model::event_structure::{BLOCK_SIZE, BLOCK_SIZE_AREA};
use crate::codec::compressed::source_model::HandleEvent;
use crate::{AbsoluteT, DeltaT, Event, EventCoordless, PixelAddress};
use std::collections::HashMap;

pub struct EventCube {
    /// The absolute y-coordinate of the top-left pixel in the cube
    pub(crate) start_y: PixelAddress,

    /// The absolute x-coordinate of the top-left pixel in the cube
    pub(crate) start_x: PixelAddress,

    /// Contains the sparse events in the cube. The index is the relative interval of dt_ref from the start
    raw_event_lists: [[[Vec<(u8, EventCoordless)>; BLOCK_SIZE]; BLOCK_SIZE]; 3],

    /// The absolute time of the cube's beginning (not necessarily aligned to an event. We structure
    /// cubes to be in temporal lockstep at the beginning.)
    start_t: AbsoluteT,

    /// How many ticks each input interval spans
    dt_ref: DeltaT,

    /// How many dt_ref intervals the whole cube spans
    num_intervals: usize,

    raw_event_memory: [[[EventCoordless; BLOCK_SIZE]; BLOCK_SIZE]; 3],

    skip_cube: bool,
}

impl EventCube {
    pub fn new(
        start_y: PixelAddress,
        start_x: PixelAddress,
        start_t: AbsoluteT,
        dt_ref: DeltaT,
        num_intervals: usize,
    ) -> Self {
        let row: [Vec<(u8, EventCoordless)>; BLOCK_SIZE] =
            vec![Vec::with_capacity(num_intervals); BLOCK_SIZE]
                .try_into()
                .unwrap();
        let square: [[Vec<(u8, EventCoordless)>; BLOCK_SIZE]; BLOCK_SIZE] =
            vec![row; BLOCK_SIZE].try_into().unwrap();
        let lists = [square.clone(), square.clone(), square.clone()];

        Self {
            start_y,
            start_x,
            raw_event_lists: lists,
            start_t,
            dt_ref,
            num_intervals,
            raw_event_memory: [[[EventCoordless::default(); BLOCK_SIZE]; BLOCK_SIZE]; 3],
            skip_cube: true,
        }
    }

    /// Clear out the cube's events and increment the start time by the cube's duration
    fn clear(&mut self) {
        for c in 0..3 {
            for y in 0..BLOCK_SIZE {
                for x in 0..BLOCK_SIZE {
                    self.raw_event_lists[c][y][x].clear();
                }
            }
        }
        self.start_t += self.num_intervals as AbsoluteT * self.dt_ref;
        self.skip_cube = true;
    }
}

impl HandleEvent for EventCube {
    /// Take in a raw event and place it at the appropriate location. Also convert the time to a
    /// deltaT representation if it's the first event for the pixel during this cube.
    ///
    /// Assume that the event does fit within the cube's time frame. This is checked at the caller.
    fn ingest_event(&mut self, mut event: Event) {
        self.skip_cube = false;
        event.coord.y -= self.start_y;
        event.coord.x -= self.start_x;

        // let convert_time = self.raw_event_lists[event.coord.c_usize()][event.coord.y_usize()]
        //     [event.coord.x_usize()]
        // .is_empty();
        // if convert_time {
        //     // If it's the first event for the pixel during this cube, then convert it to a deltaT time representation (for later intra coding)
        //     event.t -= self.raw_event_memory[event.coord.c_usize()][event.coord.y_usize()]
        //         [event.coord.x_usize()]
        //     .t;
        // }

        let index = if event.t < self.start_t {
            0
        } else {
            ((event.t - self.start_t) / self.dt_ref) as u8
        };

        self.raw_event_lists[event.coord.c_usize()][event.coord.y_usize()][event.coord.x_usize()]
            .push((
                index, // The index: the relative interval of dt_ref from the start
                EventCoordless::from(event),
            ));

        // if convert_time {
        //     // If it's the first event for the pixel during this cube, convert the time back for the memory
        //     event.t += self.raw_event_memory[event.coord.c_usize()][event.coord.y_usize()]
        //         [event.coord.x_usize()]
        //     .t;
        // }

        self.raw_event_memory[event.coord.c_usize()][event.coord.y_usize()]
            [event.coord.x_usize()] = EventCoordless::from(event);
    }

    fn digest_event(&mut self) {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use super::EventCube;
    use crate::codec::compressed::source_model::HandleEvent;
    use crate::{Coord, Event, PixelAddress};

    /// Create an empty cube
    #[test]
    fn create_cube() -> Result<(), Box<dyn std::error::Error>> {
        let cube = EventCube::new(16, 16, 255, 255, 2550);
        assert_eq!(cube.start_y, 16);
        assert_eq!(cube.start_x, 16);

        Ok(())
    }

    /// Create a cube and add several sparse events to it
    fn fill_cube() -> Result<EventCube, Box<dyn std::error::Error>> {
        let mut cube = EventCube::new(16, 16, 255, 255, 2550);
        assert_eq!(cube.start_y, 16);
        assert_eq!(cube.start_x, 16);

        cube.ingest_event(Event {
            coord: Coord {
                x: 27,
                y: 17,
                c: None,
            },
            t: 280,
            d: 7,
        });

        cube.ingest_event(Event {
            coord: Coord {
                x: 27,
                y: 17,
                c: None,
            },
            t: 285,
            d: 7,
        });

        cube.ingest_event(Event {
            coord: Coord {
                x: 29,
                y: 17,
                c: None,
            },
            t: 290,
            d: 7,
        });

        Ok(cube)
    }
    #[test]
    fn test_fill_cube() -> Result<(), Box<dyn std::error::Error>> {
        let cube = fill_cube()?;
        assert!(cube.raw_event_lists[0][0][0].is_empty());
        assert_eq!(cube.raw_event_lists[0][1][13].len(), 1);

        Ok(())
    }

    #[test]
    fn fill_second_cube() -> Result<(), Box<dyn std::error::Error>> {
        let mut cube = fill_cube()?;
        cube.clear();
        assert_eq!(cube.raw_event_lists[0][1][13].len(), 0);
        cube.ingest_event(Event {
            coord: Coord {
                x: 29,
                y: 17,
                c: None,
            },
            t: 500,
            d: 7,
        });
        assert_eq!(cube.raw_event_lists[0][1][13].len(), 1);
        Ok(())
    }
}
