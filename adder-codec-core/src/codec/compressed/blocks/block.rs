use crate::codec::compressed::blocks::{
    ac_q, dc_q, Coefficient, DResidual, DeltaTResidual, EventResidual, BLOCK_SIZE, BLOCK_SIZE_AREA,
    D_ENCODE_NO_EVENT,
};
use crate::{Coord, DeltaT, Event, EventCoordless, D, D_NO_EVENT};
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
    max_dt: DeltaT,
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

    /// Perform intra-block compression.
    ///
    /// First, get prediction residuals of all event D-values and DeltaT. Then, quantize the
    /// residual DeltaT.
    ///
    /// Write the qparam. Write the D-residuals directly, because we don't want any loss. Write the
    /// quantized DeltaT residuals. Use arithmetic encoding.
    ///
    /// TODO: Note: under this, the maximum value of dtm is 8388608 (since 8388608 * BLOCK_SIZE_AREA = i32::MAX)
    fn get_intra_residual_transforms(
        &mut self,
        qparam: Option<u8>,
        dtm: DeltaT,
    ) -> ([DResidual; 256], Coefficient, [i16; 256], i16) {
        // Loop through the events and get prediction residuals

        let mut d_residuals = [D_ENCODE_NO_EVENT; BLOCK_SIZE_AREA];
        let mut dt_residuals: [Coefficient; BLOCK_SIZE_AREA] = [0.0; BLOCK_SIZE_AREA];
        let mut init = false;
        let mut start_dt: Coefficient = 0.0;

        for (idx, event_opt) in self.events.iter().enumerate() {
            if let Some(prev) = event_opt {
                // If this is the first event encountered, then encode it directly
                if !init {
                    init = true;
                    d_residuals[idx] = prev.d as DResidual;
                    // dt_residuals[idx] = prev.delta_t as Coefficient;
                    start_dt = prev.delta_t as Coefficient;
                }

                // Get the prediction residual for the next event and store it
                for (next_idx, next_event_opt) in self.events.iter().skip(idx + 1).enumerate() {
                    if let Some(next) = next_event_opt {
                        let residual = predict_residual_from_prev(prev, next, dtm);
                        d_residuals[next_idx + idx + 1] = residual.d;
                        // dt_residuals[next_idx + idx + 1] = residual.delta_t as Coefficient;
                        dt_residuals[next_idx + idx + 1] = next.delta_t as Coefficient - start_dt;
                        break;
                    }
                }
            }
        }

        // Quantize the dt residuals
        let mut planner = DctPlanner::new(); // TODO: reuse planner
        let dct = planner.plan_dct2(BLOCK_SIZE);

        //// Perform forward DCT
        dt_residuals.chunks_exact_mut(BLOCK_SIZE).for_each(|row| {
            dct.process_dct2(row);
        });

        let mut transpose_buffer = vec![0.0; BLOCK_SIZE];
        transpose::transpose_inplace(
            &mut dt_residuals,
            &mut transpose_buffer,
            BLOCK_SIZE,
            BLOCK_SIZE,
        );

        dt_residuals.chunks_exact_mut(BLOCK_SIZE).for_each(|row| {
            dct.process_dct2(row);
        });
        transpose::transpose_inplace(
            &mut dt_residuals,
            &mut transpose_buffer,
            BLOCK_SIZE,
            BLOCK_SIZE,
        );
        //// End forward DCT

        // TODO: derive qparam from the maximum delta_t in the block, so that we can use a
        // variable qparam for each block and keep the range of symbols small.
        //// Quantize the coefficients

        // Test if any of the coefficients are too large
        let max_coeff = dt_residuals
            .iter()
            .map(|x| x.abs())
            .fold(0.0, |acc: f32, x| acc.max(x));
        let mut qp_dt = 0;
        if max_coeff > i16::MAX as f32 {
            qp_dt = (max_coeff / i16::MAX as f32) as i16 + 1;
        }

        // for elem in dt_residuals.iter() {
        //     if *elem > i16::MAX as f32 || *elem < i16::MIN as f32 {
        //         panic!("Coefficient too large: {}", elem);
        //     }
        // }
        // let mut qp_dt: i16 = 0;
        // if self.max_dt * BLOCK_SIZE_AREA as DeltaT > i16::MAX as DeltaT {
        //     // panic!("DeltaT too large: {}", self.max_dt);
        //     qp_dt = ((self.max_dt as u32 * BLOCK_SIZE_AREA as u32) / i16::MAX as u32) as i16 + 1;
        // }

        let mut arr_i32: [i32; BLOCK_SIZE_AREA] = dt_residuals
            .iter()
            .map(|x| *x as i32)
            .collect::<Vec<i32>>()
            .try_into()
            .unwrap();
        // let pre_quantized = arr_i16.clone();
        // assume 12-bit depth
        let mut dc_quant = match qparam {
            None => 1 + qp_dt,
            Some(q) => dc_q(q, 0, 12) + qp_dt,
        };
        arr_i32[0] = arr_i32[0] / dc_quant as i32;

        let mut ac_quant = match qparam {
            None => 1 + qp_dt,
            Some(q) => ac_q(q, 0, 12) + qp_dt,
        };
        for elem in arr_i32.iter_mut().skip(1) {
            *elem = *elem / ac_quant as i32;
        }
        let mut arr_i16: [i16; BLOCK_SIZE_AREA] = arr_i32
            .iter()
            .map(|x| *x as i16)
            .collect::<Vec<i16>>()
            .try_into()
            .unwrap();
        //// End quantize the coefficients

        (d_residuals, start_dt, arr_i16, qp_dt)
    }

    /// Takes in the quantized DeltaT residuals and dequantizes them, performs inverse DCT, and
    /// returns the reconstructed events from the residuals.
    fn get_intra_residual_inverse(
        &mut self,
        qparam: Option<u8>,
        dtm: DeltaT,
        d_residuals: [DResidual; BLOCK_SIZE_AREA],
        start_dt: Coefficient,
        mut dt_residuals: [i16; BLOCK_SIZE_AREA],
        qp_dt: i16,
    ) -> [Option<EventCoordless>; BLOCK_SIZE_AREA] {
        let divisor = 4.0 / (BLOCK_SIZE_AREA as f64);

        let mut dt_residuals: [i32; BLOCK_SIZE_AREA] = dt_residuals
            .iter()
            .map(|x| *x as i32)
            .collect::<Vec<i32>>()
            .try_into()
            .unwrap();

        let mut dc_quant = match qparam {
            None => 1,
            Some(q) => dc_q(q, 0, 12),
        } + qp_dt;
        dt_residuals[0] = dt_residuals[0] * dc_quant as i32;

        let mut ac_quant = match qparam {
            None => 1,
            Some(q) => ac_q(q, 0, 12),
        } + qp_dt;

        for elem in dt_residuals.iter_mut().skip(1) {
            *elem = *elem * ac_quant as i32;
        }

        let mut dt_coeffs = dt_residuals
            .iter()
            .map(|x| *x as f64 * divisor)
            .collect::<Vec<f64>>();

        //// Perform inverse DCT
        let mut planner = DctPlanner::new(); // TODO: reuse planner
        let dct = planner.plan_dct2(BLOCK_SIZE);
        dt_coeffs.chunks_exact_mut(BLOCK_SIZE).for_each(|row| {
            dct.process_dct3(row);
        });
        let mut transpose_buffer = vec![0.0; BLOCK_SIZE];
        transpose::transpose_inplace(
            &mut dt_coeffs,
            &mut transpose_buffer,
            BLOCK_SIZE,
            BLOCK_SIZE,
        );

        dt_coeffs.chunks_exact_mut(BLOCK_SIZE).for_each(|row| {
            dct.process_dct3(row);
        });
        transpose::transpose_inplace(
            &mut dt_coeffs,
            &mut transpose_buffer,
            BLOCK_SIZE,
            BLOCK_SIZE,
        );
        //// End inverse DCT

        let mut events = [None; BLOCK_SIZE_AREA];
        let mut init = false;
        // TODO!

        let mut prev = &None;
        for (idx, (d_resid, dt_resid)) in d_residuals.iter().zip(dt_coeffs).enumerate() {
            if !init && *d_resid != D_ENCODE_NO_EVENT as i16 {
                init = true;
                events[idx] = Some(EventCoordless {
                    d: *d_resid as D,
                    delta_t: start_dt as DeltaT,
                });
                prev = &events[idx];
            } else if *d_resid != D_ENCODE_NO_EVENT as i16 {
                let next = EventResidual {
                    d: *d_resid,
                    delta_t: dt_resid as DeltaTResidual,
                };
                events[idx] = Some(predict_next_from_residual(prev, &next, dtm));
                events[idx].as_mut().unwrap().delta_t = (start_dt as f64 + dt_resid) as DeltaT;
                prev = &events[idx];
            }
        }
        events
    }

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
    let delta_t_resid = next.delta_t as DeltaTResidual
        - match d_resid {
            1_i16..=20_16 => {
                // If D has increased by a little bit,
                if d_resid as u32 <= previous.delta_t.leading_zeros() / 2 {
                    min(
                        (previous.delta_t << d_resid) as DeltaTResidual,
                        dtm as DeltaTResidual,
                    )
                } else {
                    previous.delta_t as DeltaTResidual
                }
            }
            -20_i16..=-1_i16 => {
                if -d_resid as u32 <= 32 - previous.delta_t.leading_zeros() {
                    max(
                        (previous.delta_t >> -d_resid) as DeltaTResidual,
                        previous.delta_t as DeltaTResidual,
                    )
                } else {
                    previous.delta_t as DeltaTResidual
                }
            }
            // If D has not changed, or has changed a whole lot, use the previous delta_t
            _ => previous.delta_t as DeltaTResidual,
        };
    EventResidual {
        d: d_resid,
        delta_t: delta_t_resid,
    }
}

fn predict_next_from_residual(
    previous: &Option<EventCoordless>,
    next_residual: &EventResidual,
    dtm: DeltaT,
) -> EventCoordless {
    let previous = previous.as_ref().unwrap();
    let d_resid = next_residual.d;

    let delta_t: DeltaT = min(
        max(
            (next_residual.delta_t
                + match d_resid {
                    1_i16..=20_16 => {
                        // If D has increased by a little bit,
                        if d_resid as u32 <= previous.delta_t.leading_zeros() / 2 {
                            min(
                                (previous.delta_t << d_resid) as DeltaTResidual,
                                dtm as DeltaTResidual,
                            )
                        } else {
                            previous.delta_t as DeltaTResidual
                        }
                    }
                    -20_i16..=-1_i16 => {
                        if -d_resid as u32 <= 32 - previous.delta_t.leading_zeros() {
                            max(
                                (previous.delta_t >> -d_resid) as DeltaTResidual,
                                previous.delta_t as DeltaTResidual,
                            )
                        } else {
                            previous.delta_t as DeltaTResidual
                        }
                    }
                    // If D has not changed, or has changed a whole lot, use the previous delta_t
                    _ => previous.delta_t as DeltaTResidual,
                }),
            0,
        ) as DeltaT,
        dtm,
    );

    debug_assert!(delta_t <= dtm);

    EventCoordless {
        d: (previous.d as DResidual + d_resid) as D,
        delta_t: delta_t as DeltaT,
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
        let cube_width = ((width as f64) / (BLOCK_SIZE as f64)).ceil() as usize;
        let cube_height = ((height as f64) / (BLOCK_SIZE as f64)).ceil() as usize;
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
    use crate::codec::compressed::blocks::{BLOCK_SIZE, BLOCK_SIZE_AREA};
    use crate::codec::decoder::Decoder;
    use crate::codec::encoder::Encoder;
    use crate::codec::raw::stream::{RawInput, RawOutput};
    use crate::codec::{CodecError, ReadCompression, WriteCompression};
    use crate::{Coord, DeltaT, Event};
    use bitstream_io::{BigEndian, BitReader};
    use rand::prelude::StdRng;
    use rand::{Rng, SeedableRng};
    use std::fs::File;
    use std::io::{BufReader, BufWriter, Write};

    fn setup_frame(events: Vec<Event>, width: usize, height: usize) -> Frame {
        let mut frame = Frame::new(width, height, true);

        for event in events {
            frame.add_event(event).unwrap();
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
        let frame = setup_frame(events, 640, 480);
    }

    /// Test that cubes are growing correctlly, according to the incoming events.
    #[test]
    fn test_cube_growth() {
        let events = get_random_events(None, 100000, 640, 480, 3, 25500);
        let frame = setup_frame(events.clone(), 640, 480);

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

    #[test]
    fn test_intra_compression_lossless_1() {
        let dtm = 25500;
        let events = get_random_events(
            Some(743822),
            10,
            BLOCK_SIZE as u16,
            BLOCK_SIZE as u16,
            1,
            dtm,
        );
        let mut frame = setup_frame(events.clone(), BLOCK_SIZE, BLOCK_SIZE);
        for mut cube in &mut frame.cubes {
            for block in &mut cube.blocks_r {
                assert!(block.fill_count <= BLOCK_SIZE_AREA as u16);
                let (d_residuals, start_dt, dt_residuals, qp_dt) =
                    block.get_intra_residual_transforms(None, dtm);
                // dbg!(d_residuals);
                // dbg!(dt_residuals);
                let events = block.get_intra_residual_inverse(
                    None,
                    dtm,
                    d_residuals,
                    start_dt,
                    dt_residuals,
                    qp_dt,
                );

                let epsilon = 100;
                for (idx, recon_event) in events.iter().enumerate() {
                    let orig_event = block.events[idx];
                    if recon_event.is_some() && orig_event.is_some() {
                        assert_eq!(recon_event.unwrap().d, orig_event.unwrap().d);
                        assert!(
                            recon_event.unwrap().delta_t + epsilon > orig_event.unwrap().delta_t
                                && recon_event.unwrap().delta_t - epsilon
                                    < orig_event.unwrap().delta_t
                        );
                    } else {
                        assert!(recon_event.is_none() && orig_event.is_none());
                    }
                    // assert_eq!(*recon_event, orig_event);
                }
            }
        }
    }

    // Note: it's not perfectly lossless, because of the large dtm value.
    #[test]
    fn test_intra_compression_lossless_2() {
        let dtm = 25500;
        let events = get_random_events(
            Some(743822),
            10000,
            BLOCK_SIZE as u16,
            BLOCK_SIZE as u16,
            1,
            dtm,
        );
        let mut frame = setup_frame(events.clone(), BLOCK_SIZE, BLOCK_SIZE);
        for mut cube in &mut frame.cubes {
            for block in &mut cube.blocks_r {
                assert!(block.fill_count <= BLOCK_SIZE_AREA as u16);
                let (d_residuals, start_dt, dt_residuals, qp_dt) =
                    block.get_intra_residual_transforms(None, dtm);
                // dbg!(d_residuals);
                // dbg!(dt_residuals);
                let events = block.get_intra_residual_inverse(
                    None,
                    dtm,
                    d_residuals,
                    start_dt,
                    dt_residuals,
                    qp_dt,
                );

                let epsilon = 2000;
                for (idx, recon_event) in events.iter().enumerate() {
                    let orig_event = block.events[idx];
                    if recon_event.is_some() && orig_event.is_some() {
                        assert_eq!(recon_event.unwrap().d, orig_event.unwrap().d);
                        assert!(
                            recon_event.unwrap().delta_t + epsilon > orig_event.unwrap().delta_t
                                && recon_event.unwrap().delta_t.saturating_sub(epsilon)
                                    < orig_event.unwrap().delta_t
                        );
                    } else {
                        assert!(recon_event.is_none() && orig_event.is_none());
                    }
                    // assert_eq!(*recon_event, orig_event);
                }
            }
        }
    }

    // Note: it's not perfectly lossless, because of the large dtm value.
    #[test]
    fn test_intra_compression_lossless_3() {
        let dtm = 255000;
        let events = get_random_events(
            Some(743822),
            10000,
            BLOCK_SIZE as u16,
            BLOCK_SIZE as u16,
            1,
            dtm,
        );
        let mut frame = setup_frame(events.clone(), BLOCK_SIZE, BLOCK_SIZE);
        for mut cube in &mut frame.cubes {
            for block in &mut cube.blocks_r {
                assert!(block.fill_count <= BLOCK_SIZE_AREA as u16);
                let (d_residuals, start_dt, dt_residuals, qp_dt) =
                    block.get_intra_residual_transforms(None, dtm);
                // dbg!(d_residuals);
                // dbg!(dt_residuals);
                let events = block.get_intra_residual_inverse(
                    None,
                    dtm,
                    d_residuals,
                    start_dt,
                    dt_residuals,
                    qp_dt,
                );

                // As our delta_t_max value increases, we can get more loss. Increase epsilon to allow for more slop.
                let epsilon = 5000;
                for (idx, recon_event) in events.iter().enumerate() {
                    let orig_event = block.events[idx];
                    if recon_event.is_some() && orig_event.is_some() {
                        assert_eq!(recon_event.unwrap().d, orig_event.unwrap().d);
                        let recon_dt = recon_event.unwrap().delta_t;
                        let orig_dt = orig_event.unwrap().delta_t;
                        assert!(
                            recon_dt + epsilon > orig_dt
                                && recon_dt.saturating_sub(epsilon) < orig_dt
                        );
                    } else {
                        assert!(recon_event.is_none() && orig_event.is_none());
                    }
                    // assert_eq!(*recon_event, orig_event);
                }
            }
        }
    }

    #[test]
    fn test_intra_compression_lossy_1() {
        let dtm = 255000;
        let events = get_random_events(
            Some(743822),
            10000,
            BLOCK_SIZE as u16,
            BLOCK_SIZE as u16,
            1,
            dtm,
        );
        let mut frame = setup_frame(events.clone(), BLOCK_SIZE, BLOCK_SIZE);
        for mut cube in &mut frame.cubes {
            for block in &mut cube.blocks_r {
                assert!(block.fill_count <= BLOCK_SIZE_AREA as u16);
                let (d_residuals, start_dt, dt_residuals, qp_dt) =
                    block.get_intra_residual_transforms(Some(30), dtm);
                // dbg!(d_residuals);
                // dbg!(dt_residuals);
                let events = block.get_intra_residual_inverse(
                    Some(30),
                    dtm,
                    d_residuals,
                    start_dt,
                    dt_residuals,
                    qp_dt,
                );

                // As our delta_t_max value increases, we can get more loss. Increase epsilon to allow for more slop.
                let epsilon = 5000;
                for (idx, recon_event) in events.iter().enumerate() {
                    let orig_event = block.events[idx];
                    if recon_event.is_some() && orig_event.is_some() {
                        assert_eq!(recon_event.unwrap().d, orig_event.unwrap().d);
                        let recon_dt = recon_event.unwrap().delta_t;
                        let orig_dt = orig_event.unwrap().delta_t;
                        assert!(
                            recon_dt + epsilon > orig_dt
                                && recon_dt.saturating_sub(epsilon) < orig_dt
                        );
                    } else {
                        assert!(recon_event.is_none() && orig_event.is_none());
                    }
                    // assert_eq!(*recon_event, orig_event);
                }
            }
        }
    }

    #[test]
    fn test_intra_compression_lossy_1_big_frame() {
        let dtm = 255000;
        let events = get_random_events(Some(743822), 10000, 640, 480, 1, dtm);
        let mut frame = setup_frame(events.clone(), 640, 480);
        for mut cube in &mut frame.cubes {
            for block in &mut cube.blocks_r {
                assert!(block.fill_count <= BLOCK_SIZE_AREA as u16);
                let (d_residuals, start_dt, dt_residuals, qp_dt) =
                    block.get_intra_residual_transforms(Some(30), dtm);
                // dbg!(d_residuals);
                // dbg!(dt_residuals);
                let events = block.get_intra_residual_inverse(
                    Some(30),
                    dtm,
                    d_residuals,
                    start_dt,
                    dt_residuals,
                    qp_dt,
                );

                // As our delta_t_max value increases, we can get more loss. Increase epsilon to allow for more slop.
                let epsilon = 50000;
                for (idx, recon_event) in events.iter().enumerate() {
                    let orig_event = block.events[idx];
                    if recon_event.is_some() && orig_event.is_some() {
                        assert_eq!(recon_event.unwrap().d, orig_event.unwrap().d);
                        let recon_dt = recon_event.unwrap().delta_t;
                        let orig_dt = orig_event.unwrap().delta_t;
                        assert!(
                            recon_dt + epsilon > orig_dt
                                && recon_dt.saturating_sub(epsilon) < orig_dt
                        );
                    } else {
                        assert!(recon_event.is_none() && orig_event.is_none());
                    }
                    // assert_eq!(*recon_event, orig_event);
                }
            }
        }
    }

    #[test]
    fn test_real_data() {
        let mut bufreader =
            BufReader::new(File::open("/home/andrew/Downloads/test_abs.adder").unwrap());
        let mut bitreader = BitReader::endian(bufreader, BigEndian);
        let compression = <RawInput as ReadCompression<BufReader<File>>>::new();
        let mut reader = Decoder::new(Box::new(compression), &mut bitreader).unwrap();
        let mut events = Vec::new();
        loop {
            match reader.digest_event(&mut bitreader) {
                Ok(ev) => {
                    events.push(ev);
                }
                Err(_) => {
                    break;
                }
            }
        }

        let bufwriter =
            BufWriter::new(File::create("/home/andrew/Downloads/test_abs_recon.adder").unwrap());
        let compression = <RawOutput<_> as WriteCompression<BufWriter<File>>>::new(
            reader.meta().clone(),
            bufwriter,
        );
        let mut encoder: Encoder<BufWriter<File>> = Encoder::new(Box::new(compression));

        let mut frame = setup_frame(
            events.clone(),
            reader.meta().plane.w_usize(),
            reader.meta().plane.h_usize(),
        );
        let qp = 1;
        for mut cube in &mut frame.cubes {
            for block in &mut cube.blocks_r {
                assert!(block.fill_count <= BLOCK_SIZE_AREA as u16);
                let (d_residuals, start_dt, dt_residuals, qp_dt) =
                    block.get_intra_residual_transforms(None, reader.meta().delta_t_max);
                // dbg!(d_residuals);
                // dbg!(dt_residuals);
                let events = block.get_intra_residual_inverse(
                    None,
                    reader.meta().delta_t_max,
                    d_residuals,
                    start_dt,
                    dt_residuals,
                    qp_dt,
                );
                for (idx, event) in events.iter().enumerate() {
                    if event.is_some() {
                        let event_coord = Event {
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
                        encoder.ingest_event(&event_coord).unwrap();
                    }
                }

                // As our delta_t_max value increases, we can get more loss. Increase epsilon to allow for more slop.
                let epsilon = 50000;
            }
        }
        let mut writer = encoder.close_writer().unwrap().unwrap();
        writer.flush().unwrap();

        writer.into_inner().unwrap();
    }
}