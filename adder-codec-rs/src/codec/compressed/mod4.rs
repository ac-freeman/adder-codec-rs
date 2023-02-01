use crate::codec::compressed::BLOCK_SIZE_BIG;
use crate::framer::driver::EventCoordless;
use crate::Event;

// Simpler approach. Don't build a complex tree. Just group events into fixed block sizes and
// differentially encode the D-values. Choose between a block being intra- or inter-coded.
// With color sources, have 3 blocks at each idx. One for each color.
pub type BlockEvents = [Option<EventCoordless>; BLOCK_SIZE_BIG * BLOCK_SIZE_BIG];

pub struct Block3 {
    events: BlockEvents,
    block_idx_y: usize,
    block_idx_x: usize,
    block_idx_c: usize,
}

impl Block3 {
    fn new(block_idx_y: usize, block_idx_x: usize, block_idx_c: usize) -> Self {
        Self {
            events: [None; BLOCK_SIZE_BIG * BLOCK_SIZE_BIG],
            block_idx_y,
            block_idx_x,
            block_idx_c,
        }
    }

    fn set_event(&mut self, event: Event) {
        let idx = self.event_coord_to_block_idx(&event);
        let event_coordless = EventCoordless::from(event);
        self.events[idx] = Some(event_coordless);
    }
}

// TODO: use arenas to avoid allocations
pub struct Cube3 {
    blocks_r: Vec<Block3>,
    blocks_g: Vec<Block3>,
    blocks_b: Vec<Block3>,
    cube_idx_y: usize,
    cube_idx_x: usize,
    cube_idx_c: usize,
}

impl Cube3 {
    fn new(cube_idx_y: usize, cube_idx_x: usize, cube_idx_c: usize) -> Self {
        Self {
            blocks_r: Vec::new(),
            blocks_g: Vec::new(),
            blocks_b: Vec::new(),
            cube_idx_y,
            cube_idx_x,
            cube_idx_c,
        }
    }

    fn set_event(&mut self, event: Event) {
        let (idx, c) = self.event_coord_to_block_idx(&event);
        let event_coordless = EventCoordless::from(event);
        loop {
            match c {
                0 => self.blocks_r[idx].events[idx] = Some(event_coordless),
                1 => self.blocks_g[idx].events[idx] = Some(event_coordless),
                2 => self.blocks_b[idx].events[idx] = Some(event_coordless),
                _ => panic!("Invalid color"),
            }
        }
        self.events[idx] = Some(event_coordless);
    }

    #[inline(always)]
    fn event_coord_to_block_idx(&self, event: &Event) -> (usize, usize) {
        // debug_assert!(event.coord.c.unwrap_or(0) as usize == self.block_idx_c);
        let idx_y = event.coord.y as usize - (self.cube_idx_y / BLOCK_SIZE_BIG);
        let idx_x = event.coord.x as usize - (self.cube_idx_x / BLOCK_SIZE_BIG);
        (
            idx_y * BLOCK_SIZE_BIG + idx_x,
            event.coord.c.unwrap_or(0) as usize,
        );
    }
}
