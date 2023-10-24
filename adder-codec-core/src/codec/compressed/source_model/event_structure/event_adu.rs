use crate::codec::compressed::source_model::event_structure::event_cube::EventCube;
use crate::codec::compressed::source_model::event_structure::BLOCK_SIZE;
use crate::codec::compressed::source_model::HandleEvent;
use crate::{AbsoluteT, DeltaT, Event, PlaneSize};
use ndarray::Array2;

pub struct EventAdu {
    plane: PlaneSize,

    /// Contains the sparse events in the cube. The index is the relative interval of dt_ref from the start
    event_cubes: Array2<EventCube>,

    /// The absolute time of the Adu's beginning (not necessarily aligned to an event. We structure
    /// cubes to be in temporal lockstep at the beginning.)
    start_t: AbsoluteT,

    /// How many ticks each input interval spans
    dt_ref: DeltaT,

    /// How many dt_ref intervals the whole adu spans
    num_intervals: usize,

    skip_adu: bool,

    cube_to_write_count: u16,
}

impl EventAdu {
    fn new(plane: PlaneSize, start_t: AbsoluteT, dt_ref: DeltaT, num_intervals: usize) -> Self {
        Self {
            plane,
            event_cubes: Array2::from_shape_fn(
                (
                    (plane.h_usize() / BLOCK_SIZE) + 1,
                    (plane.w_usize() / BLOCK_SIZE) + 1,
                ),
                |(y, x)| {
                    EventCube::new(
                        y as u16 * BLOCK_SIZE as u16,
                        x as u16 * BLOCK_SIZE as u16,
                        plane.c_usize(),
                        start_t,
                        dt_ref,
                        num_intervals,
                    )
                },
            ),
            start_t,
            dt_ref,
            num_intervals,
            skip_adu: true,
            cube_to_write_count: 0,
        }
    }
}

impl HandleEvent for EventAdu {
    /// Take in a raw event and place it at the appropriate location.
    ///
    /// Assume that the event does fit within the adu's time frame. This is checked at the caller.
    fn ingest_event(&mut self, mut event: Event) -> bool {
        let idx_y = event.coord.y_usize() / BLOCK_SIZE;
        let idx_x = event.coord.x_usize() / BLOCK_SIZE;

        if self.event_cubes[[idx_y, idx_x]].ingest_event(event) {
            self.cube_to_write_count += 1;
        };

        return if self.skip_adu {
            self.skip_adu = false;
            true
        } else {
            false
        };
    }

    fn digest_event(&mut self) {
        todo!()
    }

    fn clear(&mut self) {
        for cube in self.event_cubes.iter_mut() {
            cube.clear();
        }
        self.skip_adu = true;
        self.cube_to_write_count = 0;
        self.start_t += self.num_intervals as AbsoluteT * self.dt_ref;
    }
}

#[cfg(test)]
mod tests {
    use crate::codec::compressed::source_model::event_structure::event_adu::EventAdu;
    use crate::codec::compressed::source_model::event_structure::BLOCK_SIZE;
    use crate::{AbsoluteT, DeltaT, PlaneSize};
    use ndarray::Array2;
    #[test]
    fn build_adu() -> Result<(), Box<dyn std::error::Error>> {
        let plane = PlaneSize::new(100, 100, 3)?;
        let start_t = 0;
        let dt_ref = 255;
        let num_intervals = 10;

        let adu = EventAdu::new(plane, start_t, dt_ref, num_intervals);

        assert_eq!(adu.event_cubes.shape(), &[7, 7]);

        Ok(())
    }

    /// Create an Adu that's 2 cubes tall, 1 cube wide
    #[test]
    fn build_tiny_adu() -> Result<(), Box<dyn std::error::Error>> {
        let plane = PlaneSize::new(16, 30, 1)?;
        let start_t = 0;
        let dt_ref = 255;
        let num_intervals = 10;

        let adu = EventAdu::new(plane, start_t, dt_ref, num_intervals);

        assert_eq!(adu.event_cubes.shape(), &[7, 7]);

        Ok(())
    }
}
