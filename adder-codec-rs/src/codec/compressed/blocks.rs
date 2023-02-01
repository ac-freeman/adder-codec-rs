use crate::codec::compressed::BLOCK_SIZE_BIG;
use crate::framer::driver::EventCoordless;
use crate::Event;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum BlockError {
    #[error("event at idx {idx:?} already exists for this block")]
    AlreadyExists { idx: usize },
}

// Simpler approach. Don't build a complex tree. Just group events into fixed block sizes and
// differentially encode the D-values. Choose between a block being intra- or inter-coded.
// With color sources, have 3 blocks at each idx. One for each color.
pub type BlockEvents = [Option<EventCoordless>; BLOCK_SIZE_BIG * BLOCK_SIZE_BIG];

pub struct Block {
    events: BlockEvents,
    fill_count: u16,
    // block_idx_y: usize,
    // block_idx_x: usize,
    // block_idx_c: usize,
}

impl Block {
    fn new(block_idx_y: usize, block_idx_x: usize, block_idx_c: usize) -> Self {
        Self {
            events: [None; BLOCK_SIZE_BIG * BLOCK_SIZE_BIG],
            // block_idx_y,
            // block_idx_x,
            // block_idx_c,
            fill_count: 0,
        }
    }

    #[inline(always)]
    fn is_filled(&self) -> bool {
        self.fill_count == (BLOCK_SIZE_BIG * BLOCK_SIZE_BIG) as u16
    }

    #[inline(always)]
    fn set_event(&mut self, event: &Event, idx: usize) -> Result<(), BlockError> {
        match self.events[idx] {
            Some(ref mut e) => return Err(BlockError::AlreadyExists { idx }),
            None => {
                self.events[idx] = Some(EventCoordless::from(*event));
                self.fill_count += 1;
            }
        }
        Ok(())
    }
}

// TODO: use arenas to avoid allocations
pub struct Cube {
    blocks_r: Vec<Block>,
    blocks_g: Vec<Block>,
    blocks_b: Vec<Block>,
    cube_idx_y: usize,
    cube_idx_x: usize,
    cube_idx_c: usize,

    /// Keeps track of the block vec index that is currently being written to for each coordinate.
    block_idx_map_r: [usize; BLOCK_SIZE_BIG * BLOCK_SIZE_BIG],
    block_idx_map_g: [usize; BLOCK_SIZE_BIG * BLOCK_SIZE_BIG],
    block_idx_map_b: [usize; BLOCK_SIZE_BIG * BLOCK_SIZE_BIG],
}

impl Cube {
    fn new(cube_idx_y: usize, cube_idx_x: usize, cube_idx_c: usize) -> Self {
        Self {
            blocks_r: vec![Block::new(0, 0, 0)],
            blocks_g: vec![Block::new(0, 0, 0)],
            blocks_b: vec![Block::new(0, 0, 0)],
            cube_idx_y,
            cube_idx_x,
            cube_idx_c,
            block_idx_map_r: [0; BLOCK_SIZE_BIG * BLOCK_SIZE_BIG],
            block_idx_map_g: [0; BLOCK_SIZE_BIG * BLOCK_SIZE_BIG],
            block_idx_map_b: [0; BLOCK_SIZE_BIG * BLOCK_SIZE_BIG],
        }
    }

    fn set_event(&mut self, event: Event) -> Result<(), BlockError> {
        let (idx, c) = self.event_coord_to_block_idx(&event);

        match c {
            0 => set_event_for_channel(&mut self.blocks_r, &mut self.block_idx_map_r, event, idx),
            1 => set_event_for_channel(&mut self.blocks_g, &mut self.block_idx_map_g, event, idx),
            2 => set_event_for_channel(&mut self.blocks_b, &mut self.block_idx_map_b, event, idx),
            _ => panic!("Invalid color"),
        }
    }

    #[inline(always)]
    fn event_coord_to_block_idx(&self, event: &Event) -> (usize, usize) {
        // debug_assert!(event.coord.c.unwrap_or(0) as usize == self.block_idx_c);
        let idx_y = event.coord.y as usize - (self.cube_idx_y / BLOCK_SIZE_BIG);
        let idx_x = event.coord.x as usize - (self.cube_idx_x / BLOCK_SIZE_BIG);
        (
            idx_y * BLOCK_SIZE_BIG + idx_x,
            event.coord.c.unwrap_or(0) as usize,
        )
    }
}

fn set_event_for_channel(
    block_vec: &mut Vec<Block>,
    block_idx_map: &mut [usize; BLOCK_SIZE_BIG * BLOCK_SIZE_BIG],
    event: Event,
    idx: usize,
) -> Result<(), BlockError> {
    if block_idx_map[idx] >= block_vec.len() {
        block_vec.push(Block::new(0, 0, 0));
    }
    match block_vec[block_idx_map[idx]].set_event(&event, idx) {
        Ok(_) => {
            block_idx_map[idx] += 1;
            Ok(())
        }
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use crate::codec::compressed::blocks::Cube;
    use crate::codec::compressed::BLOCK_SIZE_BIG;
    use crate::{Coord, Event};

    struct Setup {
        cube: Cube,
        event: Event,
        events_for_block_r: Vec<Event>,
        events_for_block_g: Vec<Event>,
        events_for_block_b: Vec<Event>,
    }
    impl Setup {
        fn new() -> Self {
            let mut events_for_block_r = Vec::new();
            for y in 0..BLOCK_SIZE_BIG {
                for x in 0..BLOCK_SIZE_BIG {
                    events_for_block_r.push(Event {
                        coord: Coord {
                            y: y as u16,
                            x: x as u16,
                            c: Some(0),
                        },
                        ..Default::default()
                    });
                }
            }

            let mut events_for_block_g = Vec::new();
            for y in 0..BLOCK_SIZE_BIG {
                for x in 0..BLOCK_SIZE_BIG {
                    events_for_block_g.push(Event {
                        coord: Coord {
                            y: y as u16,
                            x: x as u16,
                            c: Some(1),
                        },
                        ..Default::default()
                    });
                }
            }

            let mut events_for_block_b = Vec::new();
            for y in 0..BLOCK_SIZE_BIG {
                for x in 0..BLOCK_SIZE_BIG {
                    events_for_block_b.push(Event {
                        coord: Coord {
                            y: y as u16,
                            x: x as u16,
                            c: Some(2),
                        },
                        ..Default::default()
                    });
                }
            }

            Self {
                cube: Cube::new(0, 0, 0),
                event: Event {
                    coord: Coord {
                        x: 0,
                        y: 0,
                        c: Some(0),
                    },
                    d: 7,
                    delta_t: 100,
                },
                events_for_block_r,
                events_for_block_g,
                events_for_block_b,
            }
        }
    }

    #[test]
    fn test_create_cube() {
        let cube = Setup::new().cube;
        assert_eq!(cube.blocks_r.len(), 1);
        assert_eq!(cube.blocks_g.len(), 1);
        assert_eq!(cube.blocks_b.len(), 1);
    }

    #[test]
    fn test_set_event() {
        let mut setup = Setup::new();
        let mut cube = setup.cube;
        let mut event = setup.event;

        assert!(cube.set_event(event).is_ok());
        assert_eq!(cube.block_idx_map_r[0], 1);
        assert_eq!(cube.blocks_r[0].fill_count, 1);
        assert!(!cube.blocks_r[0].is_filled());
    }

    #[test]
    fn test_set_many_events() {
        let mut setup = Setup::new();
        let mut cube = setup.cube;
        let mut events = setup.events_for_block_r;

        for event in events.iter() {
            assert!(cube.set_event(event.clone()).is_ok());
        }
        assert_eq!(cube.block_idx_map_r[0], 1);
        assert_eq!(
            cube.blocks_r[0].fill_count as usize,
            BLOCK_SIZE_BIG * BLOCK_SIZE_BIG
        );

        assert!(cube.blocks_r[0].is_filled());
        assert!(!cube.blocks_g[0].is_filled());
        assert!(!cube.blocks_b[0].is_filled());

        let mut events = setup.events_for_block_g;

        for event in events.iter() {
            assert!(cube.set_event(event.clone()).is_ok());
        }
        assert!(cube.blocks_r[0].is_filled());
        assert!(cube.blocks_g[0].is_filled());
        assert!(!cube.blocks_b[0].is_filled());

        let mut events = setup.events_for_block_b;

        for event in events.iter() {
            assert!(cube.set_event(event.clone()).is_ok());
        }

        assert!(cube.blocks_r[0].is_filled());
        assert!(cube.blocks_g[0].is_filled());
        assert!(cube.blocks_b[0].is_filled());

        assert_eq!(cube.blocks_r.len(), 1);
        assert_eq!(cube.blocks_g.len(), 1);
        assert_eq!(cube.blocks_b.len(), 1);

        assert!(cube.set_event(setup.event).is_ok());

        assert_eq!(cube.blocks_r.len(), 2);
        assert_eq!(cube.blocks_g.len(), 1);
        assert_eq!(cube.blocks_b.len(), 1);
    }
}
