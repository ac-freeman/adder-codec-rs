use crate::codec::compressed::source_model::event_structure::event_cube::EventCube;
use crate::codec::compressed::source_model::event_structure::BLOCK_SIZE;
use crate::{AbsoluteT, DeltaT, PlaneSize};
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
                        start_t,
                        dt_ref,
                        num_intervals,
                    )
                },
            ),
            start_t,
            dt_ref,
        }
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
}
