use crate::codec::compressed::blocks::{BLOCK_SIZE, BLOCK_SIZE_AREA};
use crate::{Coord, Event, EventCoordless};
use std::collections::HashMap;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum BlockError {
    #[error("event at idx {idx:?} already exists for this block")]
    AlreadyExists { idx: usize },
}

// Simpler approach. Don't build a complex tree. Just group events into fixed block sizes and
// differentially encode the D-values. Choose between a block being intra- or inter-coded.
// With color sources, have 3 blocks at each idx. One for each color.
pub type BlockEvents = [Option<EventCoordless>; BLOCK_SIZE_AREA];

pub struct Block {
    /// Events organized in row-major order.
    pub events: BlockEvents,
    fill_count: u16,
    // block_idx_y: usize,
    // block_idx_x: usize,
    // block_idx_c: usize,
}

impl Block {
    fn new(_block_idx_y: usize, _block_idx_x: usize, _block_idx_c: usize) -> Self {
        Self {
            events: [None; BLOCK_SIZE_AREA],
            // block_idx_y,
            // block_idx_x,
            // block_idx_c,
            fill_count: 0,
        }
    }

    #[inline(always)]
    fn is_filled(&self) -> bool {
        self.fill_count == (BLOCK_SIZE_AREA) as u16
    }

    #[inline(always)]
    fn set_event(&mut self, event: &Event, idx: usize) -> Result<(), BlockError> {
        match self.events[idx] {
            Some(ref mut _e) => return Err(BlockError::AlreadyExists { idx }),
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
    pub blocks_r: Vec<Block>,
    pub blocks_g: Vec<Block>,
    pub blocks_b: Vec<Block>,
    cube_idx_y: usize,
    cube_idx_x: usize,
    // cube_idx_c: usize,
    /// Keeps track of the block vec index that is currently being written to for each coordinate.
    block_idx_map_r: [usize; BLOCK_SIZE_AREA],
    block_idx_map_g: [usize; BLOCK_SIZE_AREA],
    block_idx_map_b: [usize; BLOCK_SIZE_AREA],
}

impl Cube {
    pub fn new(cube_idx_y: usize, cube_idx_x: usize, cube_idx_c: usize) -> Self {
        Self {
            blocks_r: vec![Block::new(0, 0, 0)],
            blocks_g: vec![Block::new(0, 0, 0)],
            blocks_b: vec![Block::new(0, 0, 0)],
            cube_idx_y,
            cube_idx_x,
            // cube_idx_c,
            block_idx_map_r: [0; BLOCK_SIZE_AREA],
            block_idx_map_g: [0; BLOCK_SIZE_AREA],
            block_idx_map_b: [0; BLOCK_SIZE_AREA],
        }
    }

    fn set_event(&mut self, event: Event, block_idx: usize) -> Result<(), BlockError> {
        // let (idx, c) = self.event_coord_to_block_idx(&event);

        match event.coord.c.unwrap_or(0) {
            0 => set_event_for_channel(
                &mut self.blocks_r,
                &mut self.block_idx_map_r,
                event,
                block_idx,
            ),
            1 => set_event_for_channel(
                &mut self.blocks_g,
                &mut self.block_idx_map_g,
                event,
                block_idx,
            ),
            2 => set_event_for_channel(
                &mut self.blocks_b,
                &mut self.block_idx_map_b,
                event,
                block_idx,
            ),
            _ => panic!("Invalid color"),
        }
    }
}

fn set_event_for_channel(
    block_vec: &mut Vec<Block>,
    block_idx_map: &mut [usize; BLOCK_SIZE_AREA],
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

pub struct Frame {
    pub cubes: Vec<Cube>,
    pub cube_width: usize,
    pub cube_height: usize,
    pub color: bool,

    /// Maps event coordinates to their cube index and block index
    index_hashmap: HashMap<Coord, FrameToBlockIndexMap>,
}

struct FrameToBlockIndexMap {
    cube_idx: usize,
    block_idx: usize,
}

impl Frame {
    /// Creates a new compression frame with the given dimensions.
    ///
    /// # Examples
    ///
    /// ```
    /// # use adder_codec_core::codec::compressed::blocks::block::Frame;
    /// let frame = Frame::new(640, 480, true);
    /// assert_eq!(frame.cubes.len(), 1200); // 640 / 16 * 480 / 16
    /// assert_eq!(frame.cube_width, 40);
    /// assert_eq!(frame.cube_height, 30);
    /// ```
    pub fn new(width: usize, height: usize, color: bool) -> Self {
        let cube_width = ((width as f64) / (BLOCK_SIZE as f64).ceil()) as usize;
        let cube_height = ((height as f64) / (BLOCK_SIZE as f64).ceil()) as usize;
        let cube_count = cube_width * cube_height;

        let mut cubes = Vec::with_capacity(cube_count as usize);

        for y in 0..cube_height {
            for x in 0..cube_width {
                let cube = Cube::new(y, x, 0);
                cubes.push(cube);
            }
        }

        let index_hashmap = HashMap::new();

        Self {
            cubes,
            cube_width,
            cube_height,
            color,
            index_hashmap,
        }
    }

    /// Adds an event to the frame.
    /// Returns the index of the cube that the event was added to.
    /// Returns an error if the event is out of bounds.
    /// # Examples
    /// ```
    /// # use adder_codec_core::codec::compressed::blocks::block::{Frame};
    /// # use adder_codec_core::{Coord, Event};
    /// # let event = Event {
    ///             coord: Coord {
    ///                 x: 27,
    ///                 y: 13,
    ///                 c: Some(2),
    ///             },
    ///             d: 7,
    ///             delta_t: 100,
    ///         };
    /// let mut frame = Frame::new(640, 480, true);
    /// assert_eq!(frame.add_event(event).unwrap(), 1); // added to cube with idx=1
    /// ```
    pub fn add_event(&mut self, event: Event) -> Result<usize, BlockError> {
        if !self.index_hashmap.contains_key(&event.coord) {
            self.index_hashmap
                .insert(event.coord, self.event_coord_to_block_idx(&event));
        }
        let index_map = self.index_hashmap.get(&event.coord).unwrap();

        // self.event_coord_to_block_idx(&event);
        self.cubes[index_map.cube_idx].set_event(event, index_map.block_idx)?;
        Ok(index_map.cube_idx)
    }

    /// Returns the frame-level index (cube index) and the cube-level index (block index) of the event.
    #[inline(always)]
    fn event_coord_to_block_idx(&self, event: &Event) -> FrameToBlockIndexMap {
        // debug_assert!(event.coord.c.unwrap_or(0) as usize == self.block_idx_c);
        let cube_idx_y = event.coord.y as usize / BLOCK_SIZE;
        let cube_idx_x = event.coord.x as usize / BLOCK_SIZE;
        let block_idx_y = event.coord.y as usize % BLOCK_SIZE;
        let block_idx_x = event.coord.x as usize % BLOCK_SIZE;

        FrameToBlockIndexMap {
            cube_idx: cube_idx_y * self.cube_width + cube_idx_x,
            block_idx: block_idx_y * BLOCK_SIZE + block_idx_x,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::codec::compressed::blocks::block::Frame;
    use crate::codec::compressed::blocks::BLOCK_SIZE_AREA;
    use crate::{Coord, Event};

    fn setup_frame(events: Vec<Event>) -> Frame {
        let mut frame = Frame::new(640, 480, true);

        for event in events {
            frame.add_event(event).unwrap();
        }
        frame
    }

    fn get_random_events(num: usize) -> Vec<Event> {
        let mut events = Vec::with_capacity(num);
        for _ in 0..num {
            events.push(Event {
                coord: Coord {
                    x: rand::random::<u16>() % 640,
                    y: rand::random::<u16>() % 480,
                    c: Some(rand::random::<u8>() % 3),
                },
                d: rand::random::<u8>(),
                delta_t: rand::random::<u32>(),
            });
        }
        events
    }

    #[test]
    fn test_setup_frame() {
        let events = get_random_events(10000);
        let frame = setup_frame(events);
    }

    /// Test that cubes are growing correctlly, according to the incoming events.
    #[test]
    fn test_cube_growth() {
        let events = get_random_events(100000);
        let frame = setup_frame(events.clone());

        let mut cube_counts_r = vec![0; frame.cubes.len()];
        let mut cube_counts_g = vec![0; frame.cubes.len()];
        let mut cube_counts_b = vec![0; frame.cubes.len()];

        for event in events {
            let cube_idx = frame.event_coord_to_block_idx(&event).cube_idx;
            let cube_counts = match event.coord.c.unwrap_or(0) {
                0 => &mut cube_counts_r,
                1 => &mut cube_counts_g,
                2 => &mut cube_counts_b,
                _ => panic!("Invalid color"),
            };
            cube_counts[cube_idx] += 1;
        }

        for (cube_idx, cube) in frame.cubes.iter().enumerate() {
            // total fill count for r blocks
            let mut fill_count_r = 0;
            for block in &cube.blocks_r {
                assert!(block.fill_count <= BLOCK_SIZE_AREA as u16);
                fill_count_r += block.fill_count;
            }
            assert_eq!(fill_count_r, cube_counts_r[cube_idx]);

            // total fill count for g blocks
            let mut fill_count_g = 0;
            for block in &cube.blocks_g {
                assert!(block.fill_count <= BLOCK_SIZE_AREA as u16);
                fill_count_g += block.fill_count;
            }
            assert_eq!(fill_count_g, cube_counts_g[cube_idx]);

            // total fill count for b blocks
            let mut fill_count_b = 0;
            for block in &cube.blocks_b {
                assert!(block.fill_count <= BLOCK_SIZE_AREA as u16);
                fill_count_b += block.fill_count;
            }
            assert_eq!(fill_count_b, cube_counts_b[cube_idx]);
        }
    }
}
