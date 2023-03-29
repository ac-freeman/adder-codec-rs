use crate::codec::compressed::blocks::prediction::PredictionModel;
use crate::codec::compressed::blocks::{
    ac_q, dc_q, Coefficient, DResidual, DeltaTResidual, EventResidual, BLOCK_SIZE, BLOCK_SIZE_AREA,
    D_ENCODE_NO_EVENT,
};
use crate::Mode::FramePerfect;
use crate::{AbsoluteT, Coord, DeltaT, Event, EventCoordless, Mode, D};
use itertools::Itertools;
use rustdct::DctPlanner;
use std::cmp::{max, min};
use std::collections::HashMap;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum BlockError {
    #[error("event at idx {idx:?} already exists for this block")]
    AlreadyExists { idx: usize },
}

// Simpler approach. Don't build a complex tree for now. Just group events into fixed block sizes and
// differentially encode the D-values. Choose between a block being intra- or inter-coded.
// With color sources, have 3 blocks at each idx. One for each color.
pub type BlockEvents = [Option<EventCoordless>; BLOCK_SIZE_AREA];

pub struct Block {
    /// Events organized in row-major order.
    pub events: BlockEvents,
    fill_count: u16,
    max_dt: DeltaT, // TODO: remove?
                    // block_idx_y: usize,
                    // block_idx_x: usize,
                    // block_idx_c: usize,
}

impl Block {
    pub fn new() -> Self {
        Self {
            events: [None; BLOCK_SIZE_AREA],
            // block_idx_y,
            // block_idx_x,
            // block_idx_c,
            fill_count: 0,
            max_dt: 0,
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
                if event.delta_t > self.max_dt {
                    self.max_dt = event.delta_t;
                }
            }
        }
        Ok(())
    }

    // pub fn get_intra_residual_tshifts_inverse(
    //     &mut self,
    //     sparam: u8,
    //     // dtm: DeltaT,
    //     start_t: AbsoluteT,
    //     start_d: D,
    //     d_residuals: [DResidual; BLOCK_SIZE_AREA],
    //     mut t_residuals: [i16; BLOCK_SIZE_AREA],
    // ) -> [Option<EventCoordless>; BLOCK_SIZE_AREA] {
    //     let mut events: [Option<EventCoordless>; BLOCK_SIZE_AREA] = [None; BLOCK_SIZE_AREA];
    //     let mut init = false;
    //     let mut start = EventCoordless {
    //         d: start_d,
    //         delta_t: start_t,
    //     };
    //     events[0] = Some(start);
    //
    //     for ((idx, d_resid), t_resid) in d_residuals.iter().enumerate().zip(t_residuals.iter()) {
    //         if *d_resid != D_ENCODE_NO_EVENT {
    //             let next = EventCoordless {
    //                 d: (*d_resid + start.d as DResidual) as D,
    //                 delta_t: (((start.delta_t as DeltaTResidual) << sparam)
    //                     + ((*t_resid as DeltaTResidual) << sparam))
    //                     as DeltaT,
    //             };
    //             events[idx] = Some(next);
    //         }
    //     }
    //
    //     events
    // }

    fn compress_inter(&mut self) {
        todo!()
    }
}

fn predict_residual_from_prev(
    previous: &EventCoordless,
    next: &EventCoordless,
    dtm: DeltaT,
) -> EventResidual {
    /// Predict what the `next` DeltaT will be, based on the change in D and the current DeltaT
    let d_resid = next.d as DResidual - previous.d as DResidual;

    // Get the prediction error for delta_t based on the change in D
    // let delta_t_resid = next.delta_t as DeltaTResidual
    //     - match d_resid {
    //         1_i16..=20_16 => {
    //             // If D has increased by a little bit,
    //             (previous.delta_t + (dtm / d_resid as DeltaT)) as DeltaTResidual
    //
    //             // if d_resid as u32 <= previous.delta_t.leading_zeros() / 2 {
    //             //     min(
    //             //         (previous.delta_t << d_resid) as DeltaTResidual,
    //             //         dtm as DeltaTResidual,
    //             //     )
    //             // } else {
    //             //     previous.delta_t as DeltaTResidual
    //             // }
    //         }
    //         -20_i16..=-1_i16 => {
    //             (previous.delta_t - (dtm / d_resid as DeltaT)) as DeltaTResidual
    //
    //             // if -d_resid as u32 <= 32 - previous.delta_t.leading_zeros() {
    //             //     max(
    //             //         (previous.delta_t >> -d_resid) as DeltaTResidual,
    //             //         previous.delta_t as DeltaTResidual,
    //             //     )
    //             // } else {
    //             //     previous.delta_t as DeltaTResidual
    //             // }
    //         }
    //         // If D has not changed, or has changed a whole lot, use the previous delta_t
    //         _ => previous.delta_t as DeltaTResidual,
    //     };
    let delta_t_resid = next.delta_t as DeltaTResidual - previous.delta_t as DeltaTResidual;
    EventResidual {
        d: d_resid,
        delta_t: delta_t_resid,
    }
}

fn predict_next_from_residual(
    previous: &EventCoordless,
    next_residual: &EventResidual,
    dtm: DeltaT,
) -> EventCoordless {
    let d_resid = next_residual.d;
    let delta_t = (next_residual.delta_t + previous.delta_t as DeltaTResidual) as DeltaT;

    // let delta_t: DeltaT = min(
    //     max(
    //         (next_residual.delta_t
    //             + match d_resid {
    //                 1_i16..=20_16 => {
    //                     // If D has increased by a little bit,
    //                     (previous.delta_t + (dtm / d_resid as DeltaT)) as DeltaTResidual
    //                     // if d_resid as u32 <= previous.delta_t.leading_zeros() / 2 {
    //                     //     min(
    //                     //         (previous.delta_t << d_resid) as DeltaTResidual,
    //                     //         dtm as DeltaTResidual,
    //                     //     )
    //                     // } else {
    //                     //     previous.delta_t as DeltaTResidual
    //                     // }
    //                 }
    //                 -20_i16..=-1_i16 => {
    //                     (previous.delta_t - (dtm / d_resid as DeltaT)) as DeltaTResidual
    //                     // if -d_resid as u32 <= 32 - previous.delta_t.leading_zeros() {
    //                     //     max(
    //                     //         (previous.delta_t >> -d_resid) as DeltaTResidual,
    //                     //         previous.delta_t as DeltaTResidual,
    //                     //     )
    //                     // } else {
    //                     //     previous.delta_t as DeltaTResidual
    //                     // }
    //                 }
    //                 // If D has not changed, or has changed a whole lot, use the previous delta_t
    //                 _ => previous.delta_t as DeltaTResidual,
    //             }),
    //         0,
    //     ) as DeltaT,
    //     dtm,
    // );

    // debug_assert!(delta_t <= dtm);

    EventCoordless {
        d: (previous.d as DResidual + d_resid) as D,
        delta_t: delta_t as DeltaT,
    }
}

// TODO: use arenas to avoid allocations
pub struct Cube {
    pub blocks_r: Vec<Block>,
    pub inter_model_r: PredictionModel,
    pub blocks_g: Vec<Block>,
    pub blocks_b: Vec<Block>,
    pub(crate) cube_idx_y: usize,
    pub(crate) cube_idx_x: usize,
    // cube_idx_c: usize,
    /// Keeps track of the block vec index that is currently being written to for each coordinate.
    block_idx_map_r: [usize; BLOCK_SIZE_AREA],
    block_idx_map_g: [usize; BLOCK_SIZE_AREA],
    block_idx_map_b: [usize; BLOCK_SIZE_AREA],
}

impl Cube {
    pub fn new(
        cube_idx_y: usize,
        cube_idx_x: usize,
        cube_idx_c: usize,
        time_modulation_mode: Mode,
    ) -> Self {
        Self {
            blocks_r: vec![Block::new()],
            inter_model_r: PredictionModel::new(time_modulation_mode),
            blocks_g: vec![Block::new()],
            blocks_b: vec![Block::new()],
            cube_idx_y,
            cube_idx_x,
            // cube_idx_c,
            block_idx_map_r: [0; BLOCK_SIZE_AREA],
            block_idx_map_g: [0; BLOCK_SIZE_AREA],
            block_idx_map_b: [0; BLOCK_SIZE_AREA],
        }
    }

    pub fn reset(&mut self) {
        self.blocks_r.clear();
        self.blocks_r.push(Block::new());
        self.blocks_g.clear();
        self.blocks_g.push(Block::new());
        self.blocks_b.clear();
        self.blocks_b.push(Block::new());
        self.block_idx_map_r = [0; BLOCK_SIZE_AREA];
        self.block_idx_map_g = [0; BLOCK_SIZE_AREA];
        self.block_idx_map_b = [0; BLOCK_SIZE_AREA];

        // Notably, DON'T do anything to the prediction models. We need to keep the same t_memory.
        // Other than that, they are reset when the intra prediction is performed.
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
        block_vec.push(Block::new());
    }
    match block_vec[block_idx_map[idx]].set_event(&event, idx) {
        Ok(_) => {
            block_idx_map[idx] += 1;
            Ok(())
        }
        Err(e) => Err(e),
    }
}

#[derive(Default)]
pub struct Frame {
    pub cubes: Vec<Cube>,
    pub cube_width: usize,
    pub cube_height: usize,
    pub color: bool,
    start_event_t: DeltaT,
    time_modulation_mode: Mode,

    /// Maps event coordinates to their cube index and block index
    index_hashmap: HashMap<Coord, FrameToBlockIndexMap>,
}

struct FrameToBlockIndexMap {
    /// The cube's spatial index in the frame
    cube_idx: usize,

    /// The pixel's spatial index within the cube/block
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
    pub fn new(width: usize, height: usize, color: bool, time_modulation_mode: Mode) -> Self {
        let cube_width = ((width as f64) / (BLOCK_SIZE as f64)).ceil() as usize;
        let cube_height = ((height as f64) / (BLOCK_SIZE as f64)).ceil() as usize;
        let cube_count = cube_width * cube_height;

        let mut cubes = Vec::with_capacity(cube_count as usize);

        for y in 0..cube_height {
            for x in 0..cube_width {
                let cube = Cube::new(y, x, 0, time_modulation_mode);
                cubes.push(cube);
            }
        }

        let index_hashmap = HashMap::new();

        Self {
            cubes,
            cube_width,
            cube_height,
            color,
            start_event_t: 0,
            time_modulation_mode,
            index_hashmap,
        }
    }

    pub(crate) fn reset(&mut self) {
        // self.cubes.clear();
        self.start_event_t = 0;
        // self.index_hashmap.clear();
        for y in 0..self.cube_height {
            for x in 0..self.cube_width {
                let old_cube = &mut self.cubes[y * self.cube_width + x];
                old_cube.reset();

                // let cube = Cube::new(y, x, 0, self.time_modulation_mode);
                // self.cubes.push(cube);
            }
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
    /// assert_eq!(frame.add_event(event,).unwrap(), 1); // added to cube with idx=1
    /// ```
    pub fn add_event(&mut self, event: Event, dtm: DeltaT) -> Result<(bool, usize), BlockError> {
        // Used to determine if the frame is big enough that we can / need to compress it now
        let ev_t = event.delta_t;
        if self.start_event_t == 0 {
            self.start_event_t = ev_t;
        }

        if ev_t > self.start_event_t + dtm {
            // self.start_event_t = a;
            return Ok((true, 0));
        }

        if !self.index_hashmap.contains_key(&event.coord) {
            self.index_hashmap
                .insert(event.coord, self.event_coord_to_block_idx(&event));
        }
        let index_map = self.index_hashmap.get(&event.coord).unwrap();

        // self.event_coord_to_block_idx(&event);
        self.cubes[index_map.cube_idx].set_event(event, index_map.block_idx)?;

        match ev_t {
            a if self.start_event_t == 0 => {
                self.start_event_t = a;
                Ok((false, index_map.cube_idx))
            }
            // a if a > self.start_event_t + dtm => {
            //     self.start_event_t = a;
            //     Ok((true, index_map.cube_idx))
            // }
            _ => Ok((false, index_map.cube_idx)),
        }
    }

    /// Add an event that's given in delta_t mode, converting it to absolute_t mode in the process.
    pub fn add_event_dt_to_abs_t(&mut self, mut event: Event) -> Result<usize, BlockError> {
        if !self.index_hashmap.contains_key(&event.coord) {
            self.index_hashmap
                .insert(event.coord, self.event_coord_to_block_idx(&event));
        }
        let index_map = self.index_hashmap.get(&event.coord).unwrap();

        let block_num = match event.coord.c.unwrap_or(0) {
            0 => self.cubes[index_map.cube_idx].block_idx_map_r[index_map.block_idx],
            1 => self.cubes[index_map.cube_idx].block_idx_map_g[index_map.block_idx],
            2 => self.cubes[index_map.cube_idx].block_idx_map_b[index_map.block_idx],
            _ => panic!("Invalid color"),
        };

        if block_num > 0 {
            event.delta_t += match event.coord.c.unwrap_or(0) {
                0 => {
                    self.cubes[index_map.cube_idx].blocks_r
                        [self.cubes[index_map.cube_idx].block_idx_map_r[index_map.block_idx] - 1]
                        .events[index_map.block_idx]
                        .unwrap()
                        .delta_t
                }
                1 => {
                    self.cubes[index_map.cube_idx].blocks_g
                        [self.cubes[index_map.cube_idx].block_idx_map_g[index_map.block_idx] - 1]
                        .events[index_map.block_idx]
                        .unwrap()
                        .delta_t
                }
                2 => {
                    self.cubes[index_map.cube_idx].blocks_b
                        [self.cubes[index_map.cube_idx].block_idx_map_b[index_map.block_idx] - 1]
                        .events[index_map.block_idx]
                        .unwrap()
                        .delta_t
                }
                _ => panic!("Invalid color"),
            };
        }

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

    pub(crate) fn serialize_to_events(self) -> Result<Vec<Event>, BlockError> {
        let mut events = Vec::new();
        for cube in self.cubes {
            for block in cube.blocks_r {
                for (idx, event) in block.events.iter().enumerate() {
                    if event.is_some() {
                        let mut event_coorded = Event {
                            coord: Coord {
                                x: (cube.cube_idx_x * BLOCK_SIZE as usize
                                    + (idx % BLOCK_SIZE as usize))
                                    as u16,
                                y: (cube.cube_idx_y * BLOCK_SIZE as usize
                                    + (idx / BLOCK_SIZE as usize))
                                    as u16,
                                c: None,
                            },
                            d: event.unwrap().d,
                            delta_t: event.unwrap().delta_t,
                        };
                        events.push(event_coorded);
                    }
                }
            }
        }
        Ok(events)
    }
}

#[cfg(test)]
mod tests {
    use crate::codec::compressed::adu::cube::AduCube;
    use crate::codec::compressed::adu::frame::{compare_channels, Adu, AduChannelType};
    use crate::codec::compressed::adu::interblock::AduInterBlock;
    use crate::codec::compressed::adu::intrablock::AduIntraBlock;
    use crate::codec::compressed::adu::AduCompression;
    use crate::codec::compressed::blocks::block::Frame;
    use crate::codec::compressed::blocks::{BLOCK_SIZE, BLOCK_SIZE_AREA};
    use crate::codec::compressed::stream::CompressedInput;
    use crate::codec::decoder::Decoder;
    use crate::codec::encoder::Encoder;
    use crate::codec::raw::stream::{RawInput, RawOutput};
    use crate::codec::CompressedOutput;
    use crate::codec::{CodecError, ReadCompression, WriteCompression};
    use crate::Mode::{Continuous, FramePerfect};
    use crate::{Coord, DeltaT, Event, EventCoordless, Mode};
    use bitstream_io::{BigEndian, BitReader};
    use rand::prelude::StdRng;
    use rand::{Rng, SeedableRng};
    use std::fs::File;
    use std::io::{BufReader, BufWriter, Cursor, Write};

    fn setup_frame(
        events: Vec<Event>,
        width: usize,
        height: usize,
        time_modulation_mode: Mode,
        delta_t_max: DeltaT,
    ) -> Frame {
        let mut frame = Frame::new(width, height, true, time_modulation_mode);

        for event in events {
            frame.add_event(event, delta_t_max).unwrap();
        }
        frame
    }

    fn setup_frame_dt_to_abs_t(
        events: Vec<Event>,
        width: usize,
        height: usize,
        time_modulation_mode: Mode,
    ) -> Frame {
        let mut frame = Frame::new(width, height, true, time_modulation_mode);

        for event in events {
            frame.add_event_dt_to_abs_t(event).unwrap();
        }
        frame
    }

    fn get_random_events(
        seed: Option<u64>,
        num: usize,
        width: u16,
        height: u16,
        channels: u8,
        dtm: DeltaT,
    ) -> Vec<Event> {
        let mut rng = match seed {
            None => StdRng::from_rng(rand::thread_rng()).unwrap(),
            Some(num) => StdRng::seed_from_u64(num),
        };
        let mut events = Vec::with_capacity(num);
        for _ in 0..num {
            let event = Event {
                coord: Coord {
                    x: rng.gen::<u16>() % width,
                    y: rng.gen::<u16>() % height,
                    c: Some(rng.gen::<u8>() % channels),
                },
                d: rng.gen::<u8>(),
                delta_t: rng.gen::<u32>() % dtm,
            };
            events.push(event);
        }
        events
    }

    #[test]
    fn test_setup_frame() {
        let events = get_random_events(None, 10000, 640, 480, 3, 25500);
        let frame = setup_frame(events, 640, 480, Continuous, 25500);
    }

    /// Test that cubes are growing correctlly, according to the incoming events.
    #[test]
    fn test_cube_growth() {
        let events = get_random_events(None, 100000, 640, 480, 3, 25500);
        let frame = setup_frame(events.clone(), 640, 480, Continuous, 25500);

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

    // #[test]
    // fn test_intra_compression_lossless_1() {
    //     let dtm = 25500;
    //     let events = get_random_events(
    //         Some(743822),
    //         10,
    //         BLOCK_SIZE as u16,
    //         BLOCK_SIZE as u16,
    //         1,
    //         dtm,
    //     );
    //     let mut frame = setup_frame(events.clone(), BLOCK_SIZE, BLOCK_SIZE, Continuous, dtm);
    //     for mut cube in &mut frame.cubes {
    //         for block in &mut cube.blocks_r {
    //             assert!(block.fill_count <= BLOCK_SIZE_AREA as u16);
    //             let (d_residuals, start_dt, dt_residuals, qp_dt) =
    //                 block.get_intra_residual_transforms(None, dtm);
    //             // dbg!(d_residuals);
    //             // dbg!(dt_residuals);
    //             let events = block.get_intra_residual_inverse(
    //                 None,
    //                 dtm,
    //                 d_residuals,
    //                 start_dt,
    //                 dt_residuals,
    //                 qp_dt,
    //             );
    //
    //             let epsilon = 100;
    //             for (idx, recon_event) in events.iter().enumerate() {
    //                 let orig_event = block.events[idx];
    //                 if recon_event.is_some() && orig_event.is_some() {
    //                     assert_eq!(recon_event.unwrap().d, orig_event.unwrap().d);
    //                     assert!(
    //                         recon_event.unwrap().delta_t + epsilon > orig_event.unwrap().delta_t
    //                             && recon_event.unwrap().delta_t - epsilon
    //                                 < orig_event.unwrap().delta_t
    //                     );
    //                 } else {
    //                     assert!(recon_event.is_none() && orig_event.is_none());
    //                 }
    //                 // assert_eq!(*recon_event, orig_event);
    //             }
    //         }
    //     }
    // }
    //
    // // Note: it's not perfectly lossless, because of the large dtm value.
    // #[test]
    // fn test_intra_compression_lossless_2() {
    //     let dtm = 25500;
    //     let events = get_random_events(
    //         Some(743822),
    //         10000,
    //         BLOCK_SIZE as u16,
    //         BLOCK_SIZE as u16,
    //         1,
    //         dtm,
    //     );
    //     let mut frame = setup_frame(events.clone(), BLOCK_SIZE, BLOCK_SIZE, Continuous, dtm);
    //     for mut cube in &mut frame.cubes {
    //         for block in &mut cube.blocks_r {
    //             assert!(block.fill_count <= BLOCK_SIZE_AREA as u16);
    //             let (d_residuals, start_dt, dt_residuals, qp_dt) =
    //                 block.get_intra_residual_transforms(None, dtm);
    //             // dbg!(d_residuals);
    //             // dbg!(dt_residuals);
    //             let events = block.get_intra_residual_inverse(
    //                 None,
    //                 dtm,
    //                 d_residuals,
    //                 start_dt,
    //                 dt_residuals,
    //                 qp_dt,
    //             );
    //
    //             let epsilon = 2000;
    //             for (idx, recon_event) in events.iter().enumerate() {
    //                 let orig_event = block.events[idx];
    //                 if recon_event.is_some() && orig_event.is_some() {
    //                     assert_eq!(recon_event.unwrap().d, orig_event.unwrap().d);
    //                     assert!(
    //                         recon_event.unwrap().delta_t + epsilon > orig_event.unwrap().delta_t
    //                             && recon_event.unwrap().delta_t.saturating_sub(epsilon)
    //                                 < orig_event.unwrap().delta_t
    //                     );
    //                 } else {
    //                     assert!(recon_event.is_none() && orig_event.is_none());
    //                 }
    //                 // assert_eq!(*recon_event, orig_event);
    //             }
    //         }
    //     }
    // }
    //
    // // Note: it's not perfectly lossless, because of the large dtm value.
    // #[test]
    // fn test_intra_compression_lossless_3() {
    //     let dtm = 255000;
    //     let events = get_random_events(
    //         Some(743822),
    //         10000,
    //         BLOCK_SIZE as u16,
    //         BLOCK_SIZE as u16,
    //         1,
    //         dtm,
    //     );
    //     let mut frame = setup_frame(events.clone(), BLOCK_SIZE, BLOCK_SIZE, Continuous, dtm);
    //     for mut cube in &mut frame.cubes {
    //         for block in &mut cube.blocks_r {
    //             assert!(block.fill_count <= BLOCK_SIZE_AREA as u16);
    //             let (d_residuals, start_dt, dt_residuals, qp_dt) =
    //                 block.get_intra_residual_transforms(None, dtm);
    //             // dbg!(d_residuals);
    //             // dbg!(dt_residuals);
    //             let events = block.get_intra_residual_inverse(
    //                 None,
    //                 dtm,
    //                 d_residuals,
    //                 start_dt,
    //                 dt_residuals,
    //                 qp_dt,
    //             );
    //
    //             // As our delta_t_max value increases, we can get more loss. Increase epsilon to allow for more slop.
    //             let epsilon = 5000;
    //             for (idx, recon_event) in events.iter().enumerate() {
    //                 let orig_event = block.events[idx];
    //                 if recon_event.is_some() && orig_event.is_some() {
    //                     assert_eq!(recon_event.unwrap().d, orig_event.unwrap().d);
    //                     let recon_dt = recon_event.unwrap().delta_t;
    //                     let orig_dt = orig_event.unwrap().delta_t;
    //                     assert!(
    //                         recon_dt + epsilon > orig_dt
    //                             && recon_dt.saturating_sub(epsilon) < orig_dt
    //                     );
    //                 } else {
    //                     assert!(recon_event.is_none() && orig_event.is_none());
    //                 }
    //                 // assert_eq!(*recon_event, orig_event);
    //             }
    //         }
    //     }
    // }
    //
    // #[test]
    // fn test_intra_compression_lossy_1() {
    //     let dtm = 255000;
    //     let events = get_random_events(
    //         Some(743822),
    //         10000,
    //         BLOCK_SIZE as u16,
    //         BLOCK_SIZE as u16,
    //         1,
    //         dtm,
    //     );
    //     let mut frame = setup_frame(events.clone(), BLOCK_SIZE, BLOCK_SIZE, Continuous, dtm);
    //     for mut cube in &mut frame.cubes {
    //         for block in &mut cube.blocks_r {
    //             assert!(block.fill_count <= BLOCK_SIZE_AREA as u16);
    //             let (d_residuals, start_dt, dt_residuals, qp_dt) =
    //                 block.get_intra_residual_transforms(Some(30), dtm);
    //             // dbg!(d_residuals);
    //             // dbg!(dt_residuals);
    //             let events = block.get_intra_residual_inverse(
    //                 Some(30),
    //                 dtm,
    //                 d_residuals,
    //                 start_dt,
    //                 dt_residuals,
    //                 qp_dt,
    //             );
    //
    //             // As our delta_t_max value increases, we can get more loss. Increase epsilon to allow for more slop.
    //             let epsilon = 5000;
    //             for (idx, recon_event) in events.iter().enumerate() {
    //                 let orig_event = block.events[idx];
    //                 if recon_event.is_some() && orig_event.is_some() {
    //                     assert_eq!(recon_event.unwrap().d, orig_event.unwrap().d);
    //                     let recon_dt = recon_event.unwrap().delta_t;
    //                     let orig_dt = orig_event.unwrap().delta_t;
    //                     assert!(
    //                         recon_dt + epsilon > orig_dt
    //                             && recon_dt.saturating_sub(epsilon) < orig_dt
    //                     );
    //                 } else {
    //                     assert!(recon_event.is_none() && orig_event.is_none());
    //                 }
    //                 // assert_eq!(*recon_event, orig_event);
    //             }
    //         }
    //     }
    // }
    //
    // #[test]
    // fn test_intra_compression_lossy_1_big_frame() {
    //     let dtm = 255000;
    //     let events = get_random_events(Some(743822), 10000, 640, 480, 1, dtm);
    //     let mut frame = setup_frame(events.clone(), 640, 480, Continuous, dtm);
    //     for mut cube in &mut frame.cubes {
    //         for block in &mut cube.blocks_r {
    //             assert!(block.fill_count <= BLOCK_SIZE_AREA as u16);
    //             let (d_residuals, start_dt, dt_residuals, qp_dt) =
    //                 block.get_intra_residual_transforms(Some(30), dtm);
    //             // dbg!(d_residuals);
    //             // dbg!(dt_residuals);
    //             let events = block.get_intra_residual_inverse(
    //                 Some(30),
    //                 dtm,
    //                 d_residuals,
    //                 start_dt,
    //                 dt_residuals,
    //                 qp_dt,
    //             );
    //
    //             // As our delta_t_max value increases, we can get more loss. Increase epsilon to allow for more slop.
    //             let epsilon = 50000;
    //             for (idx, recon_event) in events.iter().enumerate() {
    //                 let orig_event = block.events[idx];
    //                 if recon_event.is_some() && orig_event.is_some() {
    //                     assert_eq!(recon_event.unwrap().d, orig_event.unwrap().d);
    //                     let recon_dt = recon_event.unwrap().delta_t;
    //                     let orig_dt = orig_event.unwrap().delta_t;
    //                     assert!(
    //                         recon_dt + epsilon > orig_dt
    //                             && recon_dt.saturating_sub(epsilon) < orig_dt
    //                     );
    //                 } else {
    //                     assert!(recon_event.is_none() && orig_event.is_none());
    //                 }
    //                 // assert_eq!(*recon_event, orig_event);
    //             }
    //         }
    //     }
    // }
    //
    // #[test]
    // fn test_real_data() {
    //     let mut bufreader =
    //         BufReader::new(File::open("/home/andrew/Downloads/test_abs.adder").unwrap());
    //     let mut bitreader = BitReader::endian(bufreader, BigEndian);
    //     let compression = RawInput::new();
    //     let mut reader = Decoder::new_raw(compression, &mut bitreader).unwrap();
    //     let mut events = Vec::new();
    //     loop {
    //         match reader.digest_event(&mut bitreader) {
    //             Ok(ev) => {
    //                 events.push(ev);
    //             }
    //             Err(_) => {
    //                 break;
    //             }
    //         }
    //     }
    //
    //     let bufwriter =
    //         BufWriter::new(File::create("/home/andrew/Downloads/test_abs_recon.adder").unwrap());
    //     let compression = RawOutput::new(reader.meta().clone(), bufwriter);
    //     let mut encoder: Encoder<BufWriter<File>> = Encoder::new_raw(compression);
    //
    //     let mut frame = setup_frame(
    //         events.clone(),
    //         reader.meta().plane.w_usize(),
    //         reader.meta().plane.h_usize(),
    //         FramePerfect,
    //         reader.meta().delta_t_max,
    //     );
    //     let qp = 6;
    //     for mut cube in &mut frame.cubes {
    //         for block in &mut cube.blocks_r {
    //             assert!(block.fill_count <= BLOCK_SIZE_AREA as u16);
    //             let (d_residuals, start_dt, dt_residuals, qp_dt) =
    //                 block.get_intra_residual_transforms(None, reader.meta().delta_t_max);
    //             // dbg!(d_residuals);
    //             // dbg!(dt_residuals);
    //             let events = block.get_intra_residual_inverse(
    //                 None,
    //                 reader.meta().delta_t_max,
    //                 d_residuals,
    //                 start_dt,
    //                 dt_residuals,
    //                 qp_dt,
    //             );
    //             for (idx, event) in events.iter().enumerate() {
    //                 if event.is_some() {
    //                     let event_coord = Event {
    //                         coord: Coord {
    //                             x: (cube.cube_idx_x * BLOCK_SIZE as usize
    //                                 + (idx % BLOCK_SIZE as usize))
    //                                 as u16,
    //                             y: (cube.cube_idx_y * BLOCK_SIZE as usize
    //                                 + (idx / BLOCK_SIZE as usize))
    //                                 as u16,
    //                             c: None,
    //                         },
    //                         d: event.unwrap().d,
    //                         delta_t: event.unwrap().delta_t,
    //                     };
    //                     encoder.ingest_event(event_coord).unwrap();
    //                 }
    //             }
    //
    //             // As our delta_t_max value increases, we can get more loss. Increase epsilon to allow for more slop.
    //             let epsilon = 50000;
    //         }
    //     }
    //     let mut writer = encoder.close_writer().unwrap().unwrap();
    //     writer.flush().unwrap();
    //
    //     writer.into_inner().unwrap();
    // }
    //
    // #[test]
    // fn test_real_data_tshift() {
    //     // let mut bufreader =
    //     //     BufReader::new(File::open("/home/andrew/Downloads/test_abs2.adder").unwrap());
    //     // let mut bitreader = BitReader::endian(bufreader, BigEndian);
    //     // let compression = <RawInput as ReadCompression<BufReader<File>>>::new();
    //     // let mut reader = Decoder::new(Box::new(compression), &mut bitreader).unwrap();
    //     // let mut events = Vec::new();
    //     // loop {
    //     //     match reader.digest_event(&mut bitreader) {
    //     //         Ok(ev) => {
    //     //             events.push(ev);
    //     //         }
    //     //         Err(_) => {
    //     //             break;
    //     //         }
    //     //     }
    //     // }
    //     //
    //     // let bufwriter =
    //     //     BufWriter::new(File::create("/home/andrew/Downloads/test_abs_recon2.adder").unwrap());
    //     // let compression = <RawOutput<_> as WriteCompression<BufWriter<File>>>::new(
    //     //     reader.meta().clone(),
    //     //     bufwriter,
    //     // );
    //     // let mut encoder: Encoder<BufWriter<File>> = Encoder::new(Box::new(compression));
    //     //
    //     // let mut frame = setup_frame(
    //     //     events.clone(),
    //     //     reader.meta().plane.w_usize(),
    //     //     reader.meta().plane.h_usize(),
    //     //     FramePerfect,
    //     // );
    //     // for mut cube in &mut frame.cubes {
    //     //     for block in &mut cube.blocks_r {
    //     //         assert!(block.fill_count <= BLOCK_SIZE_AREA as u16);
    //     //         let (d_residuals, start_dt, dt_residuals, sparam) =
    //     //             block.get_intra_residual_tshifts(0, reader.meta().delta_t_max);
    //     //
    //     //         let events = block.get_intra_residual_tshifts_inverse(
    //     //             sparam,
    //     //             reader.meta().delta_t_max,
    //     //             d_residuals,
    //     //             start_dt,
    //     //             dt_residuals,
    //     //         );
    //     //
    //     //         for (idx, event) in events.iter().enumerate() {
    //     //             if event.is_some() {
    //     //                 let event_coord = Event {
    //     //                     coord: Coord {
    //     //                         x: (cube.cube_idx_x * BLOCK_SIZE as usize
    //     //                             + (idx % BLOCK_SIZE as usize))
    //     //                             as u16,
    //     //                         y: (cube.cube_idx_y * BLOCK_SIZE as usize
    //     //                             + (idx / BLOCK_SIZE as usize))
    //     //                             as u16,
    //     //                         c: None,
    //     //                     },
    //     //                     d: event.unwrap().d,
    //     //                     delta_t: event.unwrap().delta_t,
    //     //                 };
    //     //                 encoder.ingest_event(&event_coord).unwrap();
    //     //             }
    //     //         }
    //     //
    //     //         // As our delta_t_max value increases, we can get more loss. Increase epsilon to allow for more slop.
    //     //         let epsilon = 5000;
    //     //     }
    //     // }
    //     // let mut writer = encoder.close_writer().unwrap().unwrap();
    //     // writer.flush().unwrap();
    //     //
    //     // writer.into_inner().unwrap();
    // }
    //
    // #[test]
    // fn test_inter_compression_lossless_tshift() {
    //     // let dtm = 2550;
    //     // let events = get_random_events(
    //     //     Some(743822),
    //     //     1000,
    //     //     BLOCK_SIZE as u16,
    //     //     BLOCK_SIZE as u16,
    //     //     1,
    //     //     dtm,
    //     // );
    //     // let mut frame = setup_frame_dt_to_abs_t(events.clone(), BLOCK_SIZE, BLOCK_SIZE, Continuous);
    //     // for mut cube in &mut frame.cubes {
    //     //     let mut block = &mut cube.blocks_r[0];
    //     //
    //     //     let mut event_memory: [EventCoordless; BLOCK_SIZE_AREA] =
    //     //         [Default::default(); BLOCK_SIZE_AREA];
    //     //     let mut t_memory: [DeltaT; BLOCK_SIZE_AREA] = [0; BLOCK_SIZE_AREA];
    //     //     let mut t_recon = t_memory.clone();
    //     //
    //     //     let mut event_memory_inverse: [EventCoordless; BLOCK_SIZE_AREA] =
    //     //         [Default::default(); BLOCK_SIZE_AREA];
    //     //     let mut t_memory_inverse: [DeltaT; BLOCK_SIZE_AREA] = [0; BLOCK_SIZE_AREA];
    //     //     let mut t_recon_inverse = t_memory_inverse.clone();
    //     //     for (idx, event) in block.events.iter().enumerate() {
    //     //         // Should only be None on the block margins beyond the frame plane
    //     //         if let Some(ev) = event {
    //     //             event_memory[idx] = *ev;
    //     //             t_memory[idx] = ev.delta_t;
    //     //             t_recon[idx] = ev.delta_t;
    //     //         }
    //     //     }
    //     //     t_memory_inverse = t_memory.clone();
    //     //     event_memory_inverse = event_memory.clone();
    //     //     t_recon_inverse = t_recon.clone();
    //     //
    //     //     assert!(block.fill_count <= BLOCK_SIZE_AREA as u16);
    //     //     let (d_residuals, start_dt, dt_residuals, sparam) =
    //     //         block.get_intra_residual_tshifts(0, dtm);
    //     //
    //     //     let events = block.get_intra_residual_tshifts_inverse(
    //     //         sparam,
    //     //         dtm,
    //     //         d_residuals,
    //     //         start_dt,
    //     //         dt_residuals,
    //     //     );
    //     //
    //     //     let epsilon = 100;
    //     //     for (idx, recon_event) in events.iter().enumerate() {
    //     //         let orig_event = block.events[idx];
    //     //         if recon_event.is_some() && orig_event.is_some() {
    //     //             assert_eq!(recon_event.unwrap().d, orig_event.unwrap().d);
    //     //             assert!(
    //     //                 recon_event.unwrap().delta_t + epsilon > orig_event.unwrap().delta_t
    //     //                     && recon_event.unwrap().delta_t.saturating_sub(epsilon)
    //     //                         < orig_event.unwrap().delta_t
    //     //             );
    //     //         } else {
    //     //             assert!(recon_event.is_none() && orig_event.is_none());
    //     //         }
    //     //         // assert_eq!(*recon_event, orig_event);
    //     //     }
    //     //
    //     //     for (block_idx, block) in cube.blocks_r.iter_mut().skip(1).enumerate() {
    //     //         let (d_residuals, start_dt, t_residuals, sparam) = block
    //     //             .get_inter_residual_tshifts(
    //     //                 &mut event_memory,
    //     //                 &mut t_memory,
    //     //                 &mut t_recon,
    //     //                 0,
    //     //                 dtm,
    //     //                 255,
    //     //             );
    //     //
    //     //         assert!(sparam == 0);
    //     //         // t_memory_inverse = t_memory.clone();
    //     //         // event_memory_inverse = event_memory.clone();
    //     //         // t_recon_inverse = t_recon.clone();
    //     //         eprint!("{}", sparam);
    //     //
    //     //         let events = block.get_inter_residual_tshifts_inverse(
    //     //             &mut event_memory_inverse,
    //     //             &mut t_recon_inverse,
    //     //             sparam,
    //     //             d_residuals,
    //     //             t_residuals,
    //     //             dtm,
    //     //             255,
    //     //             Continuous,
    //     //         );
    //     //         for (idx, recon_event) in events.iter().enumerate() {
    //     //             let orig_event = block.events[idx];
    //     //             if recon_event.is_some() && orig_event.is_some() {
    //     //                 assert_eq!(recon_event.unwrap().d, orig_event.unwrap().d);
    //     //                 // assert!(
    //     //                 //     recon_event.unwrap().delta_t + epsilon > orig_event.unwrap().delta_t
    //     //                 //         && recon_event.unwrap().delta_t.saturating_sub(epsilon)
    //     //                 //             < orig_event.unwrap().delta_t
    //     //                 // );
    //     //             } else {
    //     //                 assert!(recon_event.is_none() && orig_event.is_none());
    //     //             }
    //     //             // assert_eq!(*recon_event, orig_event);
    //     //         }
    //     //     }
    //     // }
    // }
    //
    // #[test]
    // fn test_real_data_tshift_inter() {
    //     // let mut bufreader =
    //     //     BufReader::new(File::open("/home/andrew/Downloads/test_out_abs.adder").unwrap());
    //     // let mut bitreader = BitReader::endian(bufreader, BigEndian);
    //     // let compression = <RawInput as ReadCompression<BufReader<File>>>::new();
    //     // let mut reader = Decoder::new(Box::new(compression), &mut bitreader).unwrap();
    //     // let mut events = Vec::new();
    //     // loop {
    //     //     match reader.digest_event(&mut bitreader) {
    //     //         Ok(ev) => {
    //     //             events.push(ev);
    //     //         }
    //     //         Err(_) => {
    //     //             break;
    //     //         }
    //     //     }
    //     // }
    //     //
    //     // let bufwriter =
    //     //     BufWriter::new(File::create("/home/andrew/Downloads/test_abs_recon2.adder").unwrap());
    //     // let compression = <RawOutput<_> as WriteCompression<BufWriter<File>>>::new(
    //     //     reader.meta().clone(),
    //     //     bufwriter,
    //     // );
    //     //
    //     // let mut bufrawriter = BufWriter::new(
    //     //     File::create("/home/andrew/Downloads/test_abs_compressed_raw.adder").unwrap(),
    //     // );
    //     // let mut encoder: Encoder<BufWriter<File>> = Encoder::new(Box::new(compression));
    //     //
    //     // let mut frame = setup_frame(
    //     //     events.clone(),
    //     //     reader.meta().plane.w_usize(),
    //     //     reader.meta().plane.h_usize(),
    //     //     FramePerfect,
    //     // );
    //     // let dt_ref = reader.meta().ref_interval;
    //     // let base_sparam = 4;
    //     //
    //     // for mut cube in &mut frame.cubes {
    //     //     let mut block = &mut cube.blocks_r[0];
    //     //
    //     //     let mut event_memory: [EventCoordless; BLOCK_SIZE_AREA] =
    //     //         [Default::default(); BLOCK_SIZE_AREA];
    //     //     let mut t_memory: [DeltaT; BLOCK_SIZE_AREA] = [0; BLOCK_SIZE_AREA];
    //     //     let mut t_recon = t_memory.clone();
    //     //
    //     //     let mut event_memory_inverse: [EventCoordless; BLOCK_SIZE_AREA] =
    //     //         [Default::default(); BLOCK_SIZE_AREA];
    //     //     let mut t_memory_inverse: [DeltaT; BLOCK_SIZE_AREA] = [0; BLOCK_SIZE_AREA];
    //     //     let mut t_recon_inverse = t_memory_inverse.clone();
    //     //     for (idx, event) in block.events.iter().enumerate() {
    //     //         // Should only be None on the block margins beyond the frame plane
    //     //         if let Some(ev) = event {
    //     //             event_memory[idx] = *ev;
    //     //             t_memory[idx] = ev.delta_t;
    //     //             if t_memory[idx] % dt_ref != 0 {
    //     //                 // TODO: only do this adjustment for framed sources
    //     //                 t_memory[idx] = ((t_memory[idx] / dt_ref) + 1) * dt_ref;
    //     //             }
    //     //             t_recon[idx] = t_memory[idx];
    //     //         }
    //     //     }
    //     //     t_memory_inverse = t_memory.clone();
    //     //     event_memory_inverse = event_memory.clone();
    //     //     t_recon_inverse = t_recon.clone();
    //     //
    //     //     assert!(block.fill_count <= BLOCK_SIZE_AREA as u16);
    //     //     let (d_residuals, start_dt, dt_residuals, sparam) =
    //     //         block.get_intra_residual_tshifts(base_sparam, reader.meta().delta_t_max);
    //     //     for (d_resid, dt_resid) in d_residuals.iter().zip(dt_residuals.iter()) {
    //     //         bufrawriter.write(&d_resid.to_be_bytes()).unwrap();
    //     //         bufrawriter.write(&dt_resid.to_be_bytes()).unwrap();
    //     //     }
    //     //
    //     //     let events = block.get_intra_residual_tshifts_inverse(
    //     //         sparam,
    //     //         reader.meta().delta_t_max,
    //     //         d_residuals,
    //     //         start_dt,
    //     //         dt_residuals,
    //     //     );
    //     //
    //     //     for (idx, event) in events.iter().enumerate() {
    //     //         if event.is_some() {
    //     //             let event_coord = Event {
    //     //                 coord: Coord {
    //     //                     x: (cube.cube_idx_x * BLOCK_SIZE as usize + (idx % BLOCK_SIZE as usize))
    //     //                         as u16,
    //     //                     y: (cube.cube_idx_y * BLOCK_SIZE as usize + (idx / BLOCK_SIZE as usize))
    //     //                         as u16,
    //     //                     c: None,
    //     //                 },
    //     //                 d: event.unwrap().d,
    //     //                 delta_t: event.unwrap().delta_t,
    //     //             };
    //     //             encoder.ingest_event(&event_coord).unwrap();
    //     //         }
    //     //     }
    //     //
    //     //     for block in cube.blocks_r.iter_mut().skip(1) {
    //     //         let (d_residuals, start_dt, t_residuals, sparam) = block
    //     //             .get_inter_residual_tshifts(
    //     //                 &mut event_memory,
    //     //                 &mut t_memory,
    //     //                 &mut t_recon,
    //     //                 base_sparam,
    //     //                 reader.meta().delta_t_max,
    //     //                 dt_ref,
    //     //             );
    //     //         for (d_resid, dt_resid) in d_residuals.iter().zip(dt_residuals.iter()) {
    //     //             bufrawriter.write(&d_resid.to_be_bytes()).unwrap();
    //     //             bufrawriter.write(&dt_resid.to_be_bytes()).unwrap();
    //     //         }
    //     //
    //     //         // t_memory_inverse = t_memory.clone();
    //     //         // event_memory_inverse = event_memory.clone();
    //     //         // t_recon_inverse = t_recon.clone();
    //     //         eprint!("{}", sparam);
    //     //
    //     //         let events = block.get_inter_residual_tshifts_inverse(
    //     //             &mut event_memory_inverse,
    //     //             &mut t_recon_inverse,
    //     //             sparam,
    //     //             d_residuals,
    //     //             t_residuals,
    //     //             reader.meta().delta_t_max,
    //     //             dt_ref,
    //     //             FramePerfect,
    //     //         );
    //     //         for (idx, event) in events.iter().enumerate() {
    //     //             if event.is_some() {
    //     //                 let event_coord = Event {
    //     //                     coord: Coord {
    //     //                         x: (cube.cube_idx_x * BLOCK_SIZE as usize
    //     //                             + (idx % BLOCK_SIZE as usize))
    //     //                             as u16,
    //     //                         y: (cube.cube_idx_y * BLOCK_SIZE as usize
    //     //                             + (idx / BLOCK_SIZE as usize))
    //     //                             as u16,
    //     //                         c: None,
    //     //                     },
    //     //                     d: event.unwrap().d,
    //     //                     delta_t: event.unwrap().delta_t,
    //     //                 };
    //     //                 encoder.ingest_event(&event_coord).unwrap();
    //     //             }
    //     //         }
    //     //     }
    //     // }
    //     // let mut writer = encoder.close_writer().unwrap().unwrap();
    //     // writer.flush().unwrap();
    //     //
    //     // writer.into_inner().unwrap();
    //     //
    //     // bufrawriter.flush().unwrap();
    // }
    //
    // #[test]
    // fn test_inter_compression_lossless_tshift_refactor() {
    //     let dtm = 2550;
    //     let dt_ref = 255;
    //     let events = get_random_events(
    //         Some(743822),
    //         1000,
    //         BLOCK_SIZE as u16,
    //         BLOCK_SIZE as u16,
    //         1,
    //         dtm,
    //     );
    //     let mut frame = setup_frame_dt_to_abs_t(events.clone(), BLOCK_SIZE, BLOCK_SIZE, Continuous);
    //     for mut cube in &mut frame.cubes {
    //         let mut block = &mut cube.blocks_r[0];
    //         let mut inter_model = &mut cube.inter_model_r;
    //
    //         let mut event_memory_inverse: [EventCoordless; BLOCK_SIZE_AREA] =
    //             [Default::default(); BLOCK_SIZE_AREA];
    //
    //         assert!(block.fill_count <= BLOCK_SIZE_AREA as u16);
    //         let (start_t, start_d, d_residuals, dt_residuals, sparam) =
    //             inter_model.forward_intra_prediction(0, dt_ref, dtm, &block.events);
    //
    //         let d_resids = d_residuals.clone();
    //         let dt_resids = dt_residuals.clone();
    //         let mut t_memory_inverse = inter_model.t_memory.clone();
    //         let mut event_memory_inverse = inter_model.event_memory.clone();
    //         let mut t_recon_inverse = inter_model.t_recon.clone();
    //
    //         let events = block.get_intra_residual_tshifts_inverse(
    //             sparam, dtm, start_t, start_d, d_resids, dt_resids,
    //         );
    //
    //         let epsilon = 100;
    //         for (idx, recon_event) in events.iter().enumerate() {
    //             let orig_event = block.events[idx];
    //             if recon_event.is_some() && orig_event.is_some() {
    //                 assert_eq!(recon_event.unwrap().d, orig_event.unwrap().d);
    //                 assert!(
    //                     recon_event.unwrap().delta_t + epsilon > orig_event.unwrap().delta_t
    //                         && recon_event.unwrap().delta_t.saturating_sub(epsilon)
    //                             < orig_event.unwrap().delta_t
    //                 );
    //             } else {
    //                 assert!(recon_event.is_none() && orig_event.is_none());
    //             }
    //             // assert_eq!(*recon_event, orig_event);
    //         }
    //
    //         let mut tmp_event_memory;
    //         let mut tmp_t_recon;
    //
    //         for (block_idx, block) in cube.blocks_r.iter_mut().skip(1).enumerate() {
    //             let (d_residuals, t_residuals, sparam) =
    //                 inter_model.forward_inter_prediction(0, dtm, dt_ref, &block.events);
    //             let d_resid_clone = d_residuals.clone();
    //             let t_resid_clone = t_residuals.clone();
    //
    //             tmp_event_memory = inter_model.event_memory.clone();
    //             tmp_t_recon = inter_model.t_recon.clone();
    //
    //             assert!(sparam == 0);
    //             // t_memory_inverse = t_memory.clone();
    //             // event_memory_inverse = event_memory.clone();
    //             // t_recon_inverse = t_recon.clone();
    //             eprint!("{}", sparam);
    //
    //             inter_model.override_memory(event_memory_inverse, t_recon_inverse);
    //
    //             let events = inter_model.inverse_inter_prediction(sparam, dtm, dt_ref);
    //
    //             event_memory_inverse = inter_model.event_memory.clone();
    //             t_recon_inverse = inter_model.t_recon.clone();
    //             inter_model.override_memory(tmp_event_memory, tmp_t_recon);
    //
    //             for (idx, recon_event) in events.iter().enumerate() {
    //                 let orig_event = block.events[idx];
    //                 if recon_event.is_some() && orig_event.is_some() {
    //                     assert_eq!(recon_event.unwrap().d, orig_event.unwrap().d);
    //                     // assert!(
    //                     //     recon_event.unwrap().delta_t + epsilon > orig_event.unwrap().delta_t
    //                     //         && recon_event.unwrap().delta_t.saturating_sub(epsilon)
    //                     //             < orig_event.unwrap().delta_t
    //                     // );
    //                 } else {
    //                     assert!(recon_event.is_none() && orig_event.is_none());
    //                 }
    //                 // assert_eq!(*recon_event, orig_event);
    //             }
    //         }
    //     }
    // }
    //
    // #[test]
    // fn test_real_data_tshift_inter_refactor() {
    //     let mut bufreader =
    //         BufReader::new(File::open("/home/andrew/Downloads/test_abs2.adder").unwrap());
    //     let mut bitreader = BitReader::endian(bufreader, BigEndian);
    //     let compression = RawInput::new();
    //     let mut reader = Decoder::new_raw(compression, &mut bitreader).unwrap();
    //     let mut events = Vec::new();
    //     loop {
    //         match reader.digest_event(&mut bitreader) {
    //             Ok(ev) => {
    //                 events.push(ev);
    //             }
    //             Err(_) => {
    //                 break;
    //             }
    //         }
    //     }
    //
    //     let bufwriter =
    //         BufWriter::new(File::create("/home/andrew/Downloads/test_abs_recon2.adder").unwrap());
    //     let compression = RawOutput::new(reader.meta().clone(), bufwriter);
    //
    //     let mut bufrawriter = BufWriter::new(
    //         File::create("/home/andrew/Downloads/test_abs_compressed_raw.adder").unwrap(),
    //     );
    //     let mut encoder: Encoder<BufWriter<File>> = Encoder::new_raw(compression);
    //
    //     let mut frame = setup_frame(
    //         events.clone(),
    //         reader.meta().plane.w_usize(),
    //         reader.meta().plane.h_usize(),
    //         FramePerfect,
    //         reader.meta().delta_t_max,
    //     );
    //     let dt_ref = reader.meta().ref_interval;
    //     let dtm = reader.meta().delta_t_max;
    //     let base_sparam = 4;
    //
    //     for mut cube in &mut frame.cubes {
    //         let mut block = &mut cube.blocks_r[0];
    //         let mut inter_model = &mut cube.inter_model_r;
    //
    //         let mut event_memory_inverse: [EventCoordless; BLOCK_SIZE_AREA] =
    //             [Default::default(); BLOCK_SIZE_AREA];
    //
    //         let (start_t, start_d, d_residuals, dt_residuals, sparam) =
    //             inter_model.forward_intra_prediction(0, dt_ref, dtm, &block.events);
    //
    //         // let adu_intra_block = AduIntraBlock {
    //         //     head_event_t: dt_residuals[0] as AbsoluteT,
    //         //     head_event_d: 0,
    //         //     shift_loss_param: 0,
    //         //     d_residuals: [],
    //         //     dt_residuals: [],
    //         //     event_count: 0,
    //         // }
    //
    //         let d_resids = d_residuals.clone();
    //         let dt_resids = dt_residuals.clone();
    //         let mut t_memory_inverse = inter_model.t_memory.clone();
    //         let mut event_memory_inverse = inter_model.event_memory.clone();
    //         let mut t_recon_inverse = inter_model.t_recon.clone();
    //
    //         let events = block.get_intra_residual_tshifts_inverse(
    //             sparam, dtm, start_t, start_d, d_resids, dt_resids,
    //         );
    //
    //         let epsilon = 0;
    //         for (idx, recon_event) in events.iter().enumerate() {
    //             let orig_event = block.events[idx];
    //             if recon_event.is_some() && orig_event.is_some() {
    //                 assert_eq!(recon_event.unwrap().d, orig_event.unwrap().d);
    //                 assert!(
    //                     recon_event.unwrap().delta_t + epsilon >= orig_event.unwrap().delta_t
    //                         && recon_event.unwrap().delta_t.saturating_sub(epsilon)
    //                             <= orig_event.unwrap().delta_t
    //                 );
    //             } else {
    //                 assert!(recon_event.is_none() && orig_event.is_none());
    //             }
    //             // assert_eq!(*recon_event, orig_event);
    //         }
    //
    //         for (idx, event) in events.iter().enumerate() {
    //             if event.is_some() {
    //                 let event_coord = Event {
    //                     coord: Coord {
    //                         x: (cube.cube_idx_x * BLOCK_SIZE as usize + (idx % BLOCK_SIZE as usize))
    //                             as u16,
    //                         y: (cube.cube_idx_y * BLOCK_SIZE as usize + (idx / BLOCK_SIZE as usize))
    //                             as u16,
    //                         c: None,
    //                     },
    //                     d: event.unwrap().d,
    //                     delta_t: event.unwrap().delta_t,
    //                 };
    //                 encoder.ingest_event(event_coord).unwrap();
    //             }
    //         }
    //
    //         let mut tmp_event_memory;
    //         let mut tmp_t_recon;
    //         for block in cube.blocks_r.iter_mut().skip(1) {
    //             let (d_residuals, t_residuals, sparam) =
    //                 inter_model.forward_inter_prediction(base_sparam, dtm, dt_ref, &block.events);
    //             let d_resid_clone = d_residuals.clone();
    //             let t_resid_clone = t_residuals.clone();
    //
    //             tmp_event_memory = inter_model.event_memory.clone();
    //             tmp_t_recon = inter_model.t_recon.clone();
    //
    //             // for (d_resid, dt_resid) in d_residuals.iter().zip(dt_residuals.iter()) {
    //             //     bufrawriter.write(&d_resid.to_be_bytes()).unwrap();
    //             //     bufrawriter.write(&dt_resid.to_be_bytes()).unwrap();
    //             // }
    //
    //             // t_memory_inverse = t_memory.clone();
    //             // event_memory_inverse = event_memory.clone();
    //             // t_recon_inverse = t_recon.clone();
    //             eprint!("{}", sparam);
    //
    //             inter_model.override_memory(event_memory_inverse, t_recon_inverse);
    //
    //             let events =
    //                 inter_model.inverse_inter_prediction(sparam, reader.meta().delta_t_max, dt_ref);
    //
    //             event_memory_inverse = inter_model.event_memory.clone();
    //             t_recon_inverse = inter_model.t_recon.clone();
    //             inter_model.override_memory(tmp_event_memory, tmp_t_recon);
    //
    //             for (idx, event) in events.iter().enumerate() {
    //                 if event.is_some() {
    //                     let event_coord = Event {
    //                         coord: Coord {
    //                             x: (cube.cube_idx_x * BLOCK_SIZE as usize
    //                                 + (idx % BLOCK_SIZE as usize))
    //                                 as u16,
    //                             y: (cube.cube_idx_y * BLOCK_SIZE as usize
    //                                 + (idx / BLOCK_SIZE as usize))
    //                                 as u16,
    //                             c: None,
    //                         },
    //                         d: event.unwrap().d,
    //                         delta_t: event.unwrap().delta_t,
    //                     };
    //                     encoder.ingest_event(event_coord).unwrap();
    //                 }
    //             }
    //         }
    //     }
    //     let mut writer = encoder.close_writer().unwrap().unwrap();
    //     writer.flush().unwrap();
    //
    //     writer.into_inner().unwrap();
    //
    //     bufrawriter.flush().unwrap();
    // }
    //
    // #[test]
    // fn test_real_data_tshift_inter_refactor_adu_cast() {
    //     let mut bufreader =
    //         BufReader::new(File::open("/home/andrew/Downloads/test_abs2.adder").unwrap());
    //     let mut bitreader = BitReader::endian(bufreader, BigEndian);
    //     let compression = RawInput::new();
    //     let mut reader = Decoder::new_raw(compression, &mut bitreader).unwrap();
    //     let mut events = Vec::new();
    //     loop {
    //         match reader.digest_event(&mut bitreader) {
    //             Ok(ev) => {
    //                 events.push(ev);
    //             }
    //             Err(_) => {
    //                 break;
    //             }
    //         }
    //     }
    //
    //     let bufwriter =
    //         BufWriter::new(File::create("/home/andrew/Downloads/test_abs_recon2.adder").unwrap());
    //     let compression = RawOutput::new(reader.meta().clone(), bufwriter);
    //
    //     let mut encoder: Encoder<BufWriter<File>> = Encoder::new_raw(compression);
    //
    //     let mut frame = setup_frame(
    //         events.clone(),
    //         reader.meta().plane.w_usize(),
    //         reader.meta().plane.h_usize(),
    //         FramePerfect,
    //         reader.meta().delta_t_max,
    //     );
    //     let dt_ref = reader.meta().ref_interval;
    //     let dtm = reader.meta().delta_t_max;
    //     let base_sparam = 4;
    //
    //     let mut adu = Adu::new();
    //
    //     for (cube_idx, cube) in frame.cubes.iter_mut().enumerate() {
    //         let mut block = &mut cube.blocks_r[0];
    //         let mut inter_model = &mut cube.inter_model_r;
    //
    //         let mut event_memory_inverse: [EventCoordless; BLOCK_SIZE_AREA] =
    //             [Default::default(); BLOCK_SIZE_AREA];
    //
    //         let (start_t, start_d, d_residuals, dt_residuals, sparam) =
    //             inter_model.forward_intra_prediction(0, dt_ref, dtm, &block.events);
    //
    //         if cube_idx == 0 {
    //             adu.head_event_t = start_t;
    //         }
    //
    //         let intra_block = AduIntraBlock {
    //             head_event_t: start_t,
    //             head_event_d: start_d,
    //             shift_loss_param: sparam,
    //             d_residuals: d_residuals.clone(),
    //             dt_residuals: dt_residuals.clone(),
    //         };
    //         let mut adu_cube = AduCube::from_intra_block(
    //             intra_block,
    //             cube.cube_idx_y as u16,
    //             cube.cube_idx_x as u16,
    //         );
    //
    //         let d_resids = d_residuals.clone();
    //         let dt_resids = dt_residuals.clone();
    //         let mut t_memory_inverse = inter_model.t_memory.clone();
    //         let mut event_memory_inverse = inter_model.event_memory.clone();
    //         let mut t_recon_inverse = inter_model.t_recon.clone();
    //
    //         let events = block.get_intra_residual_tshifts_inverse(
    //             sparam, dtm, start_t, start_d, d_resids, dt_resids,
    //         );
    //
    //         let epsilon = 0;
    //         for (idx, recon_event) in events.iter().enumerate() {
    //             let orig_event = block.events[idx];
    //             if recon_event.is_some() && orig_event.is_some() {
    //                 assert_eq!(recon_event.unwrap().d, orig_event.unwrap().d);
    //                 assert!(
    //                     recon_event.unwrap().delta_t + epsilon >= orig_event.unwrap().delta_t
    //                         && recon_event.unwrap().delta_t.saturating_sub(epsilon)
    //                             <= orig_event.unwrap().delta_t
    //                 );
    //             } else {
    //                 assert!(recon_event.is_none() && orig_event.is_none());
    //             }
    //             // assert_eq!(*recon_event, orig_event);
    //         }
    //
    //         for (idx, event) in events.iter().enumerate() {
    //             if event.is_some() {
    //                 let event_coord = Event {
    //                     coord: Coord {
    //                         x: (cube.cube_idx_x * BLOCK_SIZE as usize + (idx % BLOCK_SIZE as usize))
    //                             as u16,
    //                         y: (cube.cube_idx_y * BLOCK_SIZE as usize + (idx / BLOCK_SIZE as usize))
    //                             as u16,
    //                         c: None,
    //                     },
    //                     d: event.unwrap().d,
    //                     delta_t: event.unwrap().delta_t,
    //                 };
    //                 encoder.ingest_event(event_coord).unwrap();
    //             }
    //         }
    //
    //         let mut tmp_event_memory;
    //         let mut tmp_t_recon;
    //         for block in cube.blocks_r.iter_mut().skip(1) {
    //             let (d_residuals, t_residuals, sparam) =
    //                 inter_model.forward_inter_prediction(base_sparam, dtm, dt_ref, &block.events);
    //
    //             adu_cube.add_inter_block(AduInterBlock {
    //                 shift_loss_param: sparam,
    //                 d_residuals: d_residuals.clone(),
    //                 t_residuals: t_residuals.clone(),
    //             });
    //
    //             let d_resid_clone = d_residuals.clone();
    //             let t_resid_clone = t_residuals.clone();
    //
    //             tmp_event_memory = inter_model.event_memory.clone();
    //             tmp_t_recon = inter_model.t_recon.clone();
    //
    //             // for (d_resid, dt_resid) in d_residuals.iter().zip(dt_residuals.iter()) {
    //             //     bufrawriter.write(&d_resid.to_be_bytes()).unwrap();
    //             //     bufrawriter.write(&dt_resid.to_be_bytes()).unwrap();
    //             // }
    //
    //             // t_memory_inverse = t_memory.clone();
    //             // event_memory_inverse = event_memory.clone();
    //             // t_recon_inverse = t_recon.clone();
    //             eprint!("{}", sparam);
    //
    //             inter_model.override_memory(event_memory_inverse, t_recon_inverse);
    //
    //             let events =
    //                 inter_model.inverse_inter_prediction(sparam, reader.meta().delta_t_max, dt_ref);
    //
    //             event_memory_inverse = inter_model.event_memory.clone();
    //             t_recon_inverse = inter_model.t_recon.clone();
    //             inter_model.override_memory(tmp_event_memory, tmp_t_recon);
    //
    //             for (idx, event) in events.iter().enumerate() {
    //                 if event.is_some() {
    //                     let event_coord = Event {
    //                         coord: Coord {
    //                             x: (cube.cube_idx_x * BLOCK_SIZE as usize
    //                                 + (idx % BLOCK_SIZE as usize))
    //                                 as u16,
    //                             y: (cube.cube_idx_y * BLOCK_SIZE as usize
    //                                 + (idx / BLOCK_SIZE as usize))
    //                                 as u16,
    //                             c: None,
    //                         },
    //                         d: event.unwrap().d,
    //                         delta_t: event.unwrap().delta_t,
    //                     };
    //                     encoder.ingest_event(event_coord).unwrap();
    //                 }
    //             }
    //         }
    //
    //         adu.add_cube(adu_cube, AduChannelType::R);
    //     }
    //     println!("Done writing reconstructed video");
    //     let meta = encoder.meta().clone();
    //     let mut writer = encoder.close_writer().unwrap().unwrap();
    //     writer.flush().unwrap();
    //
    //     writer.into_inner().unwrap();
    //
    //     // Below is code for arithmetic-coding of the adu we generated
    //     {
    //         let mut encoder = CompressedOutput::new(meta, Vec::new());
    //
    //         assert!(adu
    //             .compress(
    //                 encoder.arithmetic_coder.as_mut().unwrap(),
    //                 encoder.contexts.as_mut().unwrap(),
    //                 encoder.stream.as_mut().unwrap(),
    //                 encoder.meta.delta_t_max,
    //                 encoder.meta.ref_interval
    //             )
    //             .is_ok());
    //
    //         let written_data = encoder.into_writer().unwrap();
    //
    //         let output_len = written_data.len();
    //         eprintln!("Output length: {}", output_len);
    //         eprintln!(
    //             "Input length: {}",
    //             File::open("/home/andrew/Downloads/test_abs_recon2.adder")
    //                 .unwrap()
    //                 .metadata()
    //                 .unwrap()
    //                 .len()
    //         );
    //
    //         let mut bufreader = BufReader::new(written_data.as_slice());
    //         let mut bitreader =
    //             bitstream_io::BitReader::endian(&mut bufreader, bitstream_io::BigEndian);
    //
    //         let mut decoder = CompressedInput::new(meta.delta_t_max, meta.ref_interval);
    //
    //         let decoded_adu =
    //             Adu::decompress(&mut decoder, &mut contexts, &mut bitreader, dtm, dt_ref);
    //
    //         decoder
    //             .arithmetic_coder
    //             .as_mut()
    //             .unwrap()
    //             .model
    //             .set_context(decoder.contexts.as_mut().unwrap().eof_context);
    //         let eof = decoder
    //             .arithmetic_coder
    //             .as_mut()
    //             .unwrap()
    //             .decode(&mut bitreader)
    //             .unwrap();
    //         assert!(eof.is_none());
    //         assert_eq!(adu.head_event_t, decoded_adu.head_event_t);
    //
    //         compare_channels(&adu.cubes_r, &decoded_adu.cubes_r);
    //         compare_channels(&adu.cubes_g, &decoded_adu.cubes_g);
    //         compare_channels(&adu.cubes_b, &decoded_adu.cubes_b);
    //     }
    // }

    #[test]
    fn test_real_data_tshift_inter_refactor_adu_cast_direct_compressor() {
        let mut bufreader =
            // BufReader::new(File::open("/home/andrew/Downloads/bunny_test.adder").unwrap());
            BufReader::new(File::open("/home/andrew/Downloads/virat_gray_fullres.adder").unwrap());
        let mut bitreader = BitReader::endian(bufreader, BigEndian);
        let compression = RawInput::new();
        let mut reader = Decoder::new_raw(compression, &mut bitreader).unwrap();

        let bufwriter = Vec::new();
        let compression = CompressedOutput::new(reader.meta().clone(), bufwriter);

        let dtm = compression.meta.delta_t_max;
        let ref_interval = compression.meta.ref_interval;
        let mut encoder: Encoder<Vec<u8>> = Encoder::new_compressed(compression);

        let mut start_event_t = 0;
        let mut compress = false;
        let mut ev_t = 0;

        let mut adus = Vec::new();

        loop {
            match reader.digest_event(&mut bitreader) {
                Ok(ev) => {
                    if let Some(adu) = encoder.ingest_event_debug(ev).unwrap() {
                        adus.push(adu);
                    }
                }
                Err(_) => break,
            }
        }

        let mut written_data = encoder.close_writer().unwrap().unwrap();
        // let mut writer = encoder.close_writer().unwrap().unwrap();
        // writer.flush().unwrap();
        // let written_data = writer.into_inner().unwrap();
        let output_len = written_data.len();

        eprintln!("Output length: {}", output_len);
        // eprintln!(
        //     "Input length: {}",
        //     File::open("/home/andrew/Downloads/bunny_test.adder")
        //         .unwrap()
        //         .metadata()
        //         .unwrap()
        //         .len()
        // );
        eprintln!(
            "Input length: {}",
            File::open("/home/andrew/Downloads/virat_gray_fullres.adder")
                .unwrap()
                .metadata()
                .unwrap()
                .len()
        );

        // Decode the compressed file
        let mut bufreader = BufReader::new(Cursor::new(written_data));
        let mut bitreader = BitReader::endian(bufreader, BigEndian);
        let compression = CompressedInput::new(dtm, ref_interval);
        let mut decoder = Decoder::new_compressed(compression, &mut bitreader).unwrap();

        let mut recon_meta = reader.meta().clone();
        let bufwriter =
            // BufWriter::new(File::create("/home/andrew/Downloads/bunny_test_recon.adder").unwrap());
            BufWriter::new(File::create("/home/andrew/Downloads/virat_gray_fullres_recon.adder").unwrap());
        let mut compressed_recon_raw_encoder =
            Encoder::new_raw(RawOutput::new(reader.meta().clone(), bufwriter));

        // todo: temporary
        let mut count = 0;
        loop {
            match decoder.digest_event_debug(&mut bitreader) {
                Ok((Some(decoded_adu), decoded_event)) => {
                    let ref_adu = &adus[count];
                    assert_eq!(ref_adu.head_event_t, decoded_adu.head_event_t);
                    compare_channels(&ref_adu.cubes_r, &decoded_adu.cubes_r);
                    compare_channels(&ref_adu.cubes_g, &decoded_adu.cubes_g);
                    compare_channels(&ref_adu.cubes_b, &decoded_adu.cubes_b);
                    compressed_recon_raw_encoder
                        .ingest_event(decoded_event)
                        .unwrap();
                    eprintln!("adu {} is identical", count);
                    // decoder.digest_event(&mut bitreader);
                    count += 1;
                    if count == adus.len() {
                        break;
                    }
                }
                Ok((None, decoded_event)) => {
                    compressed_recon_raw_encoder
                        .ingest_event(decoded_event)
                        .unwrap();
                }
                _ => {}
            }
        }
        let mut bufwriter = compressed_recon_raw_encoder.close_writer().unwrap();
        bufwriter.unwrap().flush().unwrap();
    }
}
