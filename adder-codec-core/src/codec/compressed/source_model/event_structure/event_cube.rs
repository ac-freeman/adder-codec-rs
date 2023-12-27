use crate::codec::compressed::fenwick::context_switching::FenwickModel;
use crate::codec::compressed::source_model::cabac_contexts::{
    Contexts, BITSHIFT_ENCODE_FULL, D_RESIDUAL_OFFSET,
};
use crate::codec::compressed::source_model::event_structure::BLOCK_SIZE;
use crate::codec::compressed::source_model::{ComponentCompression, HandleEvent};
use crate::codec::compressed::{DResidual, TResidual, DRESIDUAL_NO_EVENT, DRESIDUAL_SKIP_CUBE};
use crate::codec::CodecError;
use crate::{AbsoluteT, Coord, DeltaT, Event, EventCoordless, PixelAddress, D, D_EMPTY};
use arithmetic_coding_adder_dep::{Decoder, Encoder};
use bitstream_io::{BigEndian, BitReader, BitWriter};
use std::cmp::{max, min};
use std::collections::VecDeque;
use std::io::Cursor;
use std::mem::size_of;

type Pixel = Vec<EventCoordless>;

type EventLists = [[[Pixel; BLOCK_SIZE]; BLOCK_SIZE]; 3];

#[derive(PartialEq, Debug, Clone, Default)]
pub struct EventCube {
    /// The absolute y-coordinate of the top-left pixel in the cube
    pub(crate) start_y: PixelAddress,

    /// The absolute x-coordinate of the top-left pixel in the cube
    pub(crate) start_x: PixelAddress,

    num_channels: usize,

    /// Contains the sparse events in the cube. The index is the relative interval of dt_ref from the start
    pub(crate) raw_event_lists: EventLists,

    /// The absolute time of the cube's beginning (not necessarily aligned to an event. We structure
    /// cubes to be in temporal lockstep at the beginning.)
    pub(crate) start_t: AbsoluteT,

    /// How many ticks each input interval spans
    dt_ref: DeltaT,

    /// How many dt_ref intervals the whole cube spans
    num_intervals: usize,

    raw_event_memory: [[[EventCoordless; BLOCK_SIZE]; BLOCK_SIZE]; 3],

    skip_cube: bool,

    decompressed_event_queue: VecDeque<Event>,
}

impl EventCube {
    pub fn new(
        start_y: PixelAddress,
        start_x: PixelAddress,
        num_channels: usize,
        start_t: AbsoluteT,
        dt_ref: DeltaT,
        num_intervals: usize,
    ) -> Self {
        let row: [Pixel; BLOCK_SIZE] = vec![Vec::with_capacity(num_intervals); BLOCK_SIZE]
            .try_into()
            .unwrap();
        let square: [[Pixel; BLOCK_SIZE]; BLOCK_SIZE] = vec![row; BLOCK_SIZE].try_into().unwrap();
        let lists = [square.clone(), square.clone(), square.clone()];

        Self {
            start_y,
            start_x,
            num_channels,
            raw_event_lists: lists,
            start_t,
            dt_ref,
            num_intervals,
            raw_event_memory: [[[EventCoordless::default(); BLOCK_SIZE]; BLOCK_SIZE]; 3],
            skip_cube: true,
            decompressed_event_queue: Default::default(),
        }
    }
}

fn generate_t_prediction(
    idx: usize,
    mut d_residual: DResidual,
    last_delta_t: DeltaT,
    prev_event: &EventCoordless,
    num_intervals: usize,
    dt_ref: DeltaT,
    start_t: AbsoluteT,
) -> AbsoluteT {
    if idx == 1 {
        // We don't have a deltaT context
        start_t + last_delta_t as AbsoluteT
    } else {
        if d_residual.abs() > 14 {
            d_residual = 0;
        }
        if prev_event.d == D_EMPTY {
            d_residual = -1;
        }
        // We've gotten the DeltaT between the last two events. Use that
        // to form our prediction
        let delta_t_prediction: DeltaT = if d_residual < 0 {
            last_delta_t >> -d_residual
        } else {
            last_delta_t << d_residual
        };
        max(
            prev_event.t,
            prev_event.t
                + min(delta_t_prediction, (num_intervals as u8) as u32 * dt_ref) as AbsoluteT,
        )
    }
}

impl HandleEvent for EventCube {
    /// Take in a raw event and place it at the appropriate location.
    ///
    /// Assume that the event does fit within the cube's time frame. This is checked at the caller.
    ///
    /// Returns true if this is the first event the cube has ingested
    fn ingest_event(&mut self, mut event: Event) -> bool {
        event.coord.y -= self.start_y;
        event.coord.x -= self.start_x;

        let item = EventCoordless::from(event);
        self.raw_event_lists[event.coord.c_usize()][event.coord.y_usize()][event.coord.x_usize()]
            .push(item);

        if self.raw_event_lists[event.coord.c_usize()][event.coord.y_usize()][event.coord.x_usize()]
            .len()
            > 1
        {
            let last = self.raw_event_lists[event.coord.c_usize()][event.coord.y_usize()]
                [event.coord.x_usize()][self.raw_event_lists[event.coord.c_usize()]
                [event.coord.y_usize()][event.coord.x_usize()]
            .len()
                - 2];
            debug_assert!(event.t >= last.t);
        }

        self.raw_event_memory[event.coord.c_usize()][event.coord.y_usize()]
            [event.coord.x_usize()] = EventCoordless::from(event);

        return if self.skip_cube {
            self.skip_cube = false;
            true
        } else {
            false
        };
    }

    fn digest_event(&mut self) -> Result<Event, CodecError> {
        if self.skip_cube {
            return Err(CodecError::NoMoreEvents);
            // return Err(CodecError::new(
            //     "Tried to digest an event from a cube that's been skipped",
            // ));
        } else if self.decompressed_event_queue.is_empty() {
            // Then we need to convert all the cube events back into actual events and queue them up
            for c in 0..self.num_channels {
                for y in 0..BLOCK_SIZE {
                    for x in 0..BLOCK_SIZE {
                        if !self.raw_event_lists[c][y][x].is_empty() {
                            for event in self.raw_event_lists[c][y][x].iter() {
                                let event = Event {
                                    coord: Coord {
                                        x: x as PixelAddress + self.start_x,
                                        y: y as PixelAddress + self.start_y,
                                        c: if self.num_channels == 1 {
                                            None
                                        } else {
                                            Some(c as u8)
                                        },
                                    },
                                    d: event.d,
                                    t: event.t,
                                };
                                self.decompressed_event_queue.push_back(event);
                            }
                        }
                    }
                }
            }
        }

        return if let Some(event) = self.decompressed_event_queue.pop_front() {
            if self.decompressed_event_queue.is_empty() {
                self.skip_cube = true;
            }
            Ok(event)
        } else {
            return Err(CodecError::NoMoreEvents);
            // Err(CodecError::new("No events left in the queue"))
        };
    }

    /// Clear out the cube's events and increment the start time by the cube's duration
    fn clear_compression(&mut self) {
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
    fn clear_decompression(&mut self) {
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

#[cfg(test)]
mod build_tests {
    use super::EventCube;
    use crate::codec::compressed::source_model::HandleEvent;
    use crate::{Coord, Event};

    /// Create an empty cube
    #[test]
    fn create_cube() -> Result<(), Box<dyn std::error::Error>> {
        let cube = EventCube::new(16, 16, 1, 255, 255, 2550);
        assert_eq!(cube.start_y, 16);
        assert_eq!(cube.start_x, 16);

        Ok(())
    }

    /// Create a cube and add several sparse events to it
    fn fill_cube() -> Result<EventCube, Box<dyn std::error::Error>> {
        let mut cube = EventCube::new(16, 16, 1, 255, 255, 2550);
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
        cube.clear_compression();
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

impl ComponentCompression for EventCube {
    fn compress_intra(
        &mut self,
        encoder: &mut Encoder<FenwickModel, BitWriter<Vec<u8>, BigEndian>>,
        contexts: &Contexts,
        stream: &mut BitWriter<Vec<u8>, BigEndian>,
        _: Option<u8>,
    ) -> Result<(), CodecError> {
        encoder.model.set_context(contexts.d_context);
        if self.skip_cube {
            // If we're skipping this cube, just encode a NO_EVENT symbol
            let tmp = (DRESIDUAL_SKIP_CUBE + D_RESIDUAL_OFFSET) as usize;
            encoder.encode(Some(&tmp), stream).unwrap();
            // for byte in (DRESIDUAL_SKIP_CUBE).to_be_bytes().iter() {
            //     encoder.encode(Some(&(*byte as usize)), stream).unwrap();
            // }
            return Ok(()); // We're done
        }

        let mut init_event: Option<EventCoordless> = None;
        let mut d_residual = 0;

        // Intra-code the first event (if present) for each pixel in row-major order
        for c in 0..self.num_channels {
            self.raw_event_lists[c].iter_mut().for_each(|row| {
                row.iter_mut().for_each(|pixel| {
                    encoder.model.set_context(contexts.d_context);

                    if !pixel.is_empty() {
                        let event = pixel.first_mut().unwrap();

                        if let Some(init) = &mut init_event {
                            d_residual = event.d as DResidual - init.d as DResidual;
                            // Write the D residual (relative to the start_d for the first event)

                            let tmp = (d_residual + D_RESIDUAL_OFFSET) as usize;
                            encoder.encode(Some(&tmp), stream).unwrap();
                            //     for byte in d_residual.to_be_bytes().iter() {
                            //     encoder.encode(Some(&(*byte as usize)), stream).unwrap();
                            // }
                        } else {
                            // Write the first event's D directly
                            let tmp = (event.d as DResidual + D_RESIDUAL_OFFSET) as usize;
                            encoder.encode(Some(&tmp), stream).unwrap();
                            // for byte in (event.d as DResidual).to_be_bytes().iter() {
                            //     encoder.encode(Some(&(*byte as usize)), stream).unwrap();
                            // }

                            // Create the init event with t being the start_t of the cube
                            init_event = Some(EventCoordless {
                                d: event.d,
                                t: self.start_t,
                            })
                        }

                        if let Some(init) = &mut init_event {
                            // Don't do any special prediction here (yet). Just predict the same t as previously found.
                            let t_residual_i64 = event.t as i64 - init.t as i64;
                            let (bitshift_amt, t_residual) =
                                contexts.residual_to_bitshift(t_residual_i64);
                            // contexts.residual_to_bitshift2(
                            //     init.t as i64,
                            //     t_residual_i64,
                            //     event,
                            //     init,
                            //     self.dt_ref
                            // );

                            encoder.model.set_context(contexts.bitshift_context);
                            for byte in bitshift_amt.to_be_bytes().iter() {
                                encoder.encode(Some(&(*byte as usize)), stream).unwrap();
                            }

                            encoder.model.set_context(contexts.t_context);

                            if bitshift_amt == BITSHIFT_ENCODE_FULL {
                                for byte in t_residual.to_be_bytes().iter() {
                                    encoder.encode(Some(&(*byte as usize)), stream).unwrap();
                                }
                                event.t = (init.t as i64 + t_residual) as AbsoluteT;
                            } else {
                                let t_residual = t_residual as TResidual;
                                for byte in t_residual.to_be_bytes().iter() {
                                    encoder.encode(Some(&(*byte as usize)), stream).unwrap();
                                }
                                // Shift it back for the event, so we base our next prediction on the reconstructed value!
                                // if bitshift_amt != 0 {
                                event.t = (init.t as i64
                                    + ((t_residual as i64) << bitshift_amt as i64))
                                    as AbsoluteT;
                            }
                            debug_assert!(event.t < 2_u32.pow(29));

                            *init = *event;
                        } else {
                            panic!("No init event");
                        }
                    } else {
                        // Else there's no event for this pixel. Encode a NO_EVENT symbol.
                        let tmp = (DRESIDUAL_NO_EVENT + D_RESIDUAL_OFFSET) as usize;
                        encoder.encode(Some(&tmp), stream).unwrap();
                        // for byte in (DRESIDUAL_NO_EVENT).to_be_bytes().iter() {
                        //     encoder.encode(Some(&(*byte as usize)), stream).unwrap();
                        // }
                    }
                })
            })
        }
        Ok(())
    }

    fn compress_inter(
        &mut self,
        encoder: &mut Encoder<FenwickModel, BitWriter<Vec<u8>, BigEndian>>,
        contexts: &Contexts,
        stream: &mut BitWriter<Vec<u8>, BigEndian>,
        c_thresh_max: Option<u8>,
    ) -> Result<(), CodecError> {
        if self.skip_cube {
            return Ok(());
        }
        let c_thresh_max = c_thresh_max.unwrap_or(7);
        for c in 0..self.num_channels {
            self.raw_event_lists[c].iter_mut().for_each(|row| {
                row.iter_mut().for_each(|pixel| {
                    if !pixel.is_empty() {
                        let mut idx = 1;
                        let mut last_delta_t: DeltaT = 0;
                        loop {
                            encoder.model.set_context(contexts.d_context);

                            if idx < pixel.len() {
                                // TODO: don't copy the below event?
                                let prev_event = pixel[idx - 1]; // We can assume for now that this is perfectly decoded, but later we'll corrupt it according to any loss we incur
                                let event = &mut pixel[idx];

                                // Get the D residual
                                let d_residual = event.d as DResidual - prev_event.d as DResidual;
                                // Write the D residual (relative to the start_d for the first event)
                                for byte in d_residual.to_be_bytes().iter() {
                                    encoder.encode(Some(&(*byte as usize)), stream).unwrap();
                                }

                                let t_prediction = generate_t_prediction(
                                    idx,
                                    d_residual,
                                    last_delta_t,
                                    &prev_event,
                                    self.num_intervals,
                                    self.dt_ref,
                                    self.start_t,
                                );

                                // encoder.model.set_context(contexts.dtref_context);
                                let t_residual_i64 = event.t as i64 - t_prediction as i64;
                                let (bitshift_amt, t_residual) = contexts.residual_to_bitshift2(
                                    t_prediction as i64,
                                    t_residual_i64,
                                    event,
                                    &prev_event,
                                    self.dt_ref,
                                    c_thresh_max as f64,
                                );

                                encoder.model.set_context(contexts.bitshift_context);
                                for byte in bitshift_amt.to_be_bytes().iter() {
                                    encoder.encode(Some(&(*byte as usize)), stream).unwrap();
                                }

                                encoder.model.set_context(contexts.t_context);

                                if bitshift_amt == BITSHIFT_ENCODE_FULL {
                                    for byte in t_residual.to_be_bytes().iter() {
                                        encoder.encode(Some(&(*byte as usize)), stream).unwrap();
                                    }
                                    event.t = (t_prediction as i64 + t_residual) as AbsoluteT;
                                    // debug_assert!(event.t < 5000000);
                                } else {
                                    let t_residual = t_residual as TResidual;
                                    for byte in t_residual.to_be_bytes().iter() {
                                        encoder.encode(Some(&(*byte as usize)), stream).unwrap();
                                    }
                                    // Shift it back for the event, so we base our next prediction on the reconstructed value!
                                    // if bitshift_amt != 0 {
                                    event.t = (t_prediction as i64
                                        + ((t_residual as i64) << bitshift_amt as i64))
                                        as AbsoluteT;
                                    // debug_assert!(event.t < 5000000);
                                }

                                event.t = max(event.t, prev_event.t);
                                debug_assert!(event.t >= prev_event.t);
                                last_delta_t = (event.t - prev_event.t) as DeltaT;
                            } else {
                                encoder.model.set_context(contexts.d_context);
                                // Else there's no other event for this pixel. Encode a NO_EVENT symbol.
                                for byte in (DRESIDUAL_NO_EVENT).to_be_bytes().iter() {
                                    encoder.encode(Some(&(*byte as usize)), stream).unwrap();
                                }

                                break;
                            }
                            idx += 1;
                        }
                    }
                })
            })
        }
        Ok(())
    }

    fn decompress_intra(
        &mut self,
        decoder: &mut Decoder<FenwickModel, BitReader<Cursor<Vec<u8>>, BigEndian>>,
        contexts: &Contexts,
        stream: &mut BitReader<Cursor<Vec<u8>>, BigEndian>,
        start_t: AbsoluteT,
    ) {
        let mut bitshift_buffer = [0u8; 1];
        let mut t_residual_buffer = [0u8; size_of::<TResidual>()];
        let mut t_residual_full_buffer = [0u8; size_of::<i64>()];
        let mut init_event: Option<EventCoordless> = None;

        for c in 0..self.num_channels {
            for y in 0..BLOCK_SIZE {
                for x in 0..BLOCK_SIZE {
                    let pixel = &mut self.raw_event_lists[c][y][x];

                    decoder.model.set_context(contexts.d_context);

                    let tmp = decoder.decode(stream).unwrap().unwrap() as usize;
                    let d_residual = tmp as i16 - D_RESIDUAL_OFFSET;

                    if d_residual == DRESIDUAL_SKIP_CUBE {
                        pixel.clear(); // So we can skip it for intra-coding
                        self.skip_cube = true;
                        return;
                    } else if d_residual == DRESIDUAL_NO_EVENT {
                        pixel.clear(); // So we can skip it for intra-coding
                    } else {
                        let d = if let Some(init) = &mut init_event {
                            (init.d as DResidual + d_residual) as D
                        } else {
                            // There is no init event
                            init_event = Some(EventCoordless { d: 0, t: start_t });
                            self.skip_cube = false;
                            d_residual as D
                        };

                        if let Some(init) = &mut init_event {
                            // decoder.model.set_context(contexts.dtref_context);
                            // for byte in dtref_residual_buffer.iter_mut() {
                            //     *byte = decoder.decode(stream).unwrap().unwrap() as u8;
                            // }
                            // let dtref_residual = DResidual::from_be_bytes(dtref_residual_buffer);

                            decoder.model.set_context(contexts.bitshift_context);
                            for byte in bitshift_buffer.iter_mut() {
                                *byte = decoder.decode(stream).unwrap().unwrap() as u8;
                            }
                            let bitshift_amt = bitshift_buffer[0] as u8;

                            let t_residual = if bitshift_amt == BITSHIFT_ENCODE_FULL {
                                decoder.model.set_context(contexts.t_context);
                                for byte in t_residual_full_buffer.iter_mut() {
                                    *byte = decoder.decode(stream).unwrap().unwrap() as u8;
                                }
                                i64::from_be_bytes(t_residual_full_buffer)
                            } else {
                                decoder.model.set_context(contexts.t_context);
                                for byte in t_residual_buffer.iter_mut() {
                                    *byte = decoder.decode(stream).unwrap().unwrap() as u8;
                                }
                                let t_residual = TResidual::from_be_bytes(t_residual_buffer) as i64;
                                (t_residual as i64) << bitshift_amt as i64
                            };

                            init.d = (init.d as DResidual + d_residual) as D;

                            debug_assert!(init.t as i64 + t_residual as i64 >= 0);
                            init.t = (init.t as i64 + t_residual as i64) as AbsoluteT;

                            // debug_assert!(init.t < start_t + num_intervals as AbsoluteT * dt_ref);
                            pixel.push(EventCoordless { d, t: init.t });
                        } else {
                            panic!("No init event");
                        }
                    }
                }
            }
        }
    }

    fn decompress_inter(
        &mut self,
        decoder: &mut Decoder<FenwickModel, BitReader<Cursor<Vec<u8>>, BigEndian>>,
        contexts: &Contexts,
        stream: &mut BitReader<Cursor<Vec<u8>>, BigEndian>,
    ) {
        if self.skip_cube {
            return;
        }
        let mut d_residual_buffer = [0u8; size_of::<DResidual>()];
        let mut t_residual_buffer = [0u8; size_of::<TResidual>()];
        let mut t_residual_full_buffer = [0u8; size_of::<i64>()];
        let mut bitshift_buffer = [0u8; 1];

        for c in 0..self.num_channels {
            self.raw_event_lists[c].iter_mut().for_each(|row| {
                row.iter_mut().for_each(|pixel| {
                    if !pixel.is_empty() {
                        // Then look for the next events for this pixel
                        let mut idx = 1;
                        let mut last_delta_t = 0;
                        loop {
                            decoder.model.set_context(contexts.d_context);

                            for byte in d_residual_buffer.iter_mut() {
                                *byte = decoder.decode(stream).unwrap().unwrap() as u8;
                            }
                            let d_residual = DResidual::from_be_bytes(d_residual_buffer);

                            if d_residual == DRESIDUAL_NO_EVENT {
                                break; // We have all the events for this pixel now
                            }
                            debug_assert!(idx - 1 < pixel.len());
                            let prev_event = pixel[idx - 1];

                            let d = (prev_event.d as DResidual + d_residual) as D;

                            let t_prediction = generate_t_prediction(
                                idx,
                                d_residual,
                                last_delta_t,
                                &prev_event,
                                self.num_intervals,
                                self.dt_ref,
                                self.start_t,
                            );

                            decoder.model.set_context(contexts.bitshift_context);
                            for byte in bitshift_buffer.iter_mut() {
                                *byte = decoder.decode(stream).unwrap().unwrap() as u8;
                            }
                            let bitshift_amt = bitshift_buffer[0] as u8;

                            let t_residual = if bitshift_amt == BITSHIFT_ENCODE_FULL {
                                decoder.model.set_context(contexts.t_context);
                                for byte in t_residual_full_buffer.iter_mut() {
                                    *byte = decoder.decode(stream).unwrap().unwrap() as u8;
                                }
                                i64::from_be_bytes(t_residual_full_buffer) as i64
                            } else {
                                decoder.model.set_context(contexts.t_context);
                                for byte in t_residual_buffer.iter_mut() {
                                    *byte = decoder.decode(stream).unwrap().unwrap() as u8;
                                }
                                let t_residual = TResidual::from_be_bytes(t_residual_buffer) as i64;
                                (t_residual as i64) << bitshift_amt as i64
                            };

                            let t = max(
                                (t_prediction as i64 + t_residual as i64) as AbsoluteT,
                                prev_event.t,
                            );
                            debug_assert!(t >= prev_event.t);
                            last_delta_t = (t - prev_event.t) as DeltaT;
                            // debug_assert!(
                            //     t <= self.start_t + self.num_intervals as AbsoluteT * self.dt_ref
                            // );
                            pixel.push(EventCoordless { d, t });

                            idx += 1;
                        }
                    }
                });
            });
        }
    }
}

#[cfg(test)]
mod compression_tests {
    use crate::codec::compressed::fenwick::context_switching::FenwickModel;
    use crate::codec::compressed::source_model::cabac_contexts::eof_context;
    use crate::codec::compressed::source_model::event_structure::event_cube::EventCube;
    use crate::codec::compressed::source_model::{ComponentCompression, HandleEvent};
    use crate::{Coord, DeltaT, Event};
    use arithmetic_coding_adder_dep::Encoder;
    use bitstream_io::{BigEndian, BitReader, BitWriter};
    use rand::prelude::StdRng;
    use rand::{Rng, SeedableRng};
    use std::cmp::min;
    use std::error::Error;
    use std::io::Cursor;

    #[test]
    fn compress_and_decompress_intra() -> Result<(), Box<dyn Error>> {
        let mut cube = EventCube::new(0, 0, 1, 255, 255, 10);
        let mut counter = 0;
        for _ in 0..3 {
            for y in 0..16 {
                for x in 0..15 {
                    cube.ingest_event(Event {
                        coord: Coord { x, y, c: None },
                        t: 280 + counter,
                        d: 7,
                    });
                    counter += 1;
                }
            }
        }

        let bufwriter = Vec::new();
        let mut stream = BitWriter::endian(bufwriter, BigEndian);

        let mut source_model = FenwickModel::with_symbols(u16::MAX as usize, 1 << 30);
        let contexts = crate::codec::compressed::source_model::cabac_contexts::Contexts::new(
            &mut source_model,
            255,
            2550,
        );

        let mut encoder = Encoder::new(source_model);

        cube.compress_intra(&mut encoder, &contexts, &mut stream, None)?;
        eof_context(&contexts, &mut encoder, &mut stream);

        let mut source_model = FenwickModel::with_symbols(u16::MAX as usize, 1 << 30);
        let contexts = crate::codec::compressed::source_model::cabac_contexts::Contexts::new(
            &mut source_model,
            255,
            2550,
        );
        let mut decoder = arithmetic_coding_adder_dep::Decoder::new(source_model);
        let mut stream = BitReader::endian(Cursor::new(stream.into_writer()), BigEndian);

        let mut cube2 = cube.clone();

        cube2.decompress_intra(&mut decoder, &contexts, &mut stream, 255);

        for c in 0..3 {
            for y in 0..16 {
                for x in 0..16 {
                    dbg!(c, y, x);
                    dbg!(
                        &cube.raw_event_lists[c][y][x],
                        &cube2.raw_event_lists[c][y][x]
                    );
                    if !cube.raw_event_lists[c][y][x].is_empty() {
                        assert!(!cube2.raw_event_lists[c][y][x].is_empty());
                        assert_eq!(
                            cube.raw_event_lists[c][y][x][0],
                            cube2.raw_event_lists[c][y][x][0]
                        );
                    } else {
                        assert!(cube2.raw_event_lists[c][y][x].is_empty());
                    }
                }
            }
        }

        Ok(())
    }

    #[test]
    fn compress_and_decompress_inter() -> Result<(), Box<dyn Error>> {
        let mut cube = EventCube::new(0, 0, 1, 255, 255, 2);
        let mut counter = 0;
        for _ in 0..3 {
            for y in 0..16 {
                for x in 0..15 {
                    cube.ingest_event(Event {
                        coord: Coord { x, y, c: None },
                        t: min(
                            280 + counter,
                            cube.start_t + (cube.num_intervals as u32 - 1) * cube.dt_ref,
                        ),
                        d: 7,
                    });
                    counter += 1;
                }
            }
        }

        let mut rng = StdRng::seed_from_u64(1234);

        for y in 0..16 {
            for x in 0..15 {
                for _ in 0..rng.gen_range(0..3) {
                    cube.ingest_event(Event {
                        coord: Coord { x, y, c: None },
                        t: min(
                            280 + counter,
                            cube.start_t + (cube.num_intervals as u32 - 1) * cube.dt_ref,
                        ),
                        d: rng.gen_range(4..12),
                    });
                    counter += 1;
                }
            }
        }

        let bufwriter = Vec::new();
        let mut stream = BitWriter::endian(bufwriter, BigEndian);

        let mut source_model = FenwickModel::with_symbols(u16::MAX as usize, 1 << 30);
        let contexts = crate::codec::compressed::source_model::cabac_contexts::Contexts::new(
            &mut source_model,
            255,
            510,
        );

        let mut encoder = Encoder::new(source_model);

        cube.compress_intra(&mut encoder, &contexts, &mut stream, Some(0))?;
        cube.compress_inter(&mut encoder, &contexts, &mut stream, Some(0))?;

        eof_context(&contexts, &mut encoder, &mut stream);

        let mut source_model = FenwickModel::with_symbols(u16::MAX as usize, 1 << 30);
        let contexts = crate::codec::compressed::source_model::cabac_contexts::Contexts::new(
            &mut source_model,
            255,
            510,
        );
        let mut decoder = arithmetic_coding_adder_dep::Decoder::new(source_model);
        let mut stream = BitReader::endian(Cursor::new(stream.into_writer()), BigEndian);

        let mut cube2 = cube.clone();
        cube2.decompress_intra(&mut decoder, &contexts, &mut stream, 255);
        cube2.decompress_inter(&mut decoder, &contexts, &mut stream);

        for c in 0..3 {
            for y in 0..16 {
                for x in 0..16 {
                    if !cube.raw_event_lists[c][y][x].is_empty() {
                        assert!(!cube2.raw_event_lists[c][y][x].is_empty());
                        assert_eq!(
                            cube.raw_event_lists[c][y][x][0],
                            cube2.raw_event_lists[c][y][x][0]
                        );
                        for i in 0..cube.raw_event_lists[c][y][x].len() {
                            assert_eq!(
                                cube.raw_event_lists[c][y][x][i],
                                cube2.raw_event_lists[c][y][x][i]
                            );
                        }
                    } else {
                        assert!(cube2.raw_event_lists[c][y][x].is_empty());
                    }
                }
            }
        }

        Ok(())
    }

    #[test]
    fn compress_and_decompress_empty() -> Result<(), Box<dyn Error>> {
        let mut cube = EventCube::new(0, 0, 1, 255, 255, 10);

        let bufwriter = Vec::new();
        let mut stream = BitWriter::endian(bufwriter, BigEndian);

        let mut source_model = FenwickModel::with_symbols(u16::MAX as usize, 1 << 30);
        let contexts = crate::codec::compressed::source_model::cabac_contexts::Contexts::new(
            &mut source_model,
            255,
            2550,
        );

        let mut encoder = Encoder::new(source_model);

        cube.compress_intra(&mut encoder, &contexts, &mut stream, Some(0))?;
        cube.compress_inter(&mut encoder, &contexts, &mut stream, Some(0))?;

        eof_context(&contexts, &mut encoder, &mut stream);

        let mut source_model = FenwickModel::with_symbols(u16::MAX as usize, 1 << 30);
        let contexts = crate::codec::compressed::source_model::cabac_contexts::Contexts::new(
            &mut source_model,
            255,
            2550,
        );
        let mut decoder = arithmetic_coding_adder_dep::Decoder::new(source_model);
        let mut stream = BitReader::endian(Cursor::new(stream.into_writer()), BigEndian);

        let mut cube2 = cube.clone();
        cube2.decompress_intra(&mut decoder, &contexts, &mut stream, 255);
        cube2.decompress_inter(&mut decoder, &contexts, &mut stream);

        for c in 0..3 {
            for y in 0..16 {
                for x in 0..16 {
                    if !cube.raw_event_lists[c][y][x].is_empty() {
                        assert!(!cube2.raw_event_lists[c][y][x].is_empty());
                        assert_eq!(
                            cube.raw_event_lists[c][y][x][0],
                            cube2.raw_event_lists[c][y][x][0]
                        );
                        assert_eq!(
                            cube.raw_event_lists[c][y][x],
                            cube2.raw_event_lists[c][y][x]
                        );
                    } else {
                        assert!(cube2.raw_event_lists[c][y][x].is_empty());
                    }
                }
            }
        }

        Ok(())
    }

    #[test]
    fn compress_and_decompress_intra_huge_tresidual() -> Result<(), Box<dyn Error>> {
        let num_intervals = 2;
        let mut cube = EventCube::new(0, 0, 1, 255000, 255, num_intervals);

        cube.ingest_event(Event {
            coord: Coord {
                x: 3,
                y: 3,
                c: None,
            },
            t: 255001,
            d: 7,
        });
        cube.ingest_event(Event {
            coord: Coord {
                x: 4,
                y: 3,
                c: None,
            },
            t: 280,
            d: 7,
        });

        let bufwriter = Vec::new();
        let mut stream = BitWriter::endian(bufwriter, BigEndian);

        let mut source_model = FenwickModel::with_symbols(u16::MAX as usize, 1 << 30);
        let contexts = crate::codec::compressed::source_model::cabac_contexts::Contexts::new(
            &mut source_model,
            255,
            255 * num_intervals as DeltaT,
        );

        let mut encoder = Encoder::new(source_model);

        cube.compress_intra(&mut encoder, &contexts, &mut stream, Some(0))?;
        cube.compress_inter(&mut encoder, &contexts, &mut stream, Some(0))?;

        eof_context(&contexts, &mut encoder, &mut stream);

        let mut source_model = FenwickModel::with_symbols(u16::MAX as usize, 1 << 30);
        let contexts = crate::codec::compressed::source_model::cabac_contexts::Contexts::new(
            &mut source_model,
            255,
            255 * num_intervals as DeltaT,
        );
        let mut decoder = arithmetic_coding_adder_dep::Decoder::new(source_model);
        let mut stream = BitReader::endian(Cursor::new(stream.into_writer()), BigEndian);

        let mut cube2 = cube.clone();
        cube2.decompress_intra(&mut decoder, &contexts, &mut stream, 255000);
        cube2.decompress_inter(&mut decoder, &contexts, &mut stream);

        // Note that these may NOT be the original values we ingested, due to the bit shifting!
        assert_eq!(
            cube.raw_event_lists[0][3][3][0].t,
            cube2.raw_event_lists[0][3][3][0].t
        );
        assert_eq!(
            cube.raw_event_lists[0][3][4][0].t,
            cube2.raw_event_lists[0][3][4][0].t
        );

        Ok(())
    }

    #[test]
    fn compress_and_decompress_inter_huge_tresidual() -> Result<(), Box<dyn Error>> {
        let num_intervals = 2;
        let mut cube = EventCube::new(0, 0, 1, 255000, 255, num_intervals);

        cube.ingest_event(Event {
            coord: Coord {
                x: 3,
                y: 3,
                c: None,
            },
            t: 255001,
            d: 7,
        });
        cube.ingest_event(Event {
            coord: Coord {
                x: 4,
                y: 3,
                c: None,
            },
            t: 280,
            d: 7,
        });
        cube.ingest_event(Event {
            coord: Coord {
                x: 4,
                y: 3,
                c: None,
            },
            t: 255001,
            d: 7,
        });

        let bufwriter = Vec::new();
        let mut stream = BitWriter::endian(bufwriter, BigEndian);

        let mut source_model = FenwickModel::with_symbols(u16::MAX as usize, 1 << 30);
        let contexts = crate::codec::compressed::source_model::cabac_contexts::Contexts::new(
            &mut source_model,
            255,
            255 * num_intervals as DeltaT,
        );

        let mut encoder = Encoder::new(source_model);

        cube.compress_intra(&mut encoder, &contexts, &mut stream, Some(0))?;
        cube.compress_inter(&mut encoder, &contexts, &mut stream, Some(0))?;

        eof_context(&contexts, &mut encoder, &mut stream);

        let mut source_model = FenwickModel::with_symbols(u16::MAX as usize, 1 << 30);
        let contexts = crate::codec::compressed::source_model::cabac_contexts::Contexts::new(
            &mut source_model,
            255,
            255 * num_intervals as DeltaT,
        );
        let mut decoder = arithmetic_coding_adder_dep::Decoder::new(source_model);
        let mut stream = BitReader::endian(Cursor::new(stream.into_writer()), BigEndian);

        let mut cube2 = cube.clone();
        cube2.decompress_intra(&mut decoder, &contexts, &mut stream, 255000);

        cube2.decompress_inter(&mut decoder, &contexts, &mut stream);

        // Note that these may NOT be the original values we ingested, due to the bit shifting!
        assert_eq!(
            cube.raw_event_lists[0][3][3][0].t,
            cube2.raw_event_lists[0][3][3][0].t
        );
        assert_eq!(
            cube.raw_event_lists[0][3][4][0].t,
            cube2.raw_event_lists[0][3][4][0].t
        );
        assert_eq!(
            cube.raw_event_lists[0][3][4][1].t,
            cube2.raw_event_lists[0][3][4][1].t
        );

        Ok(())
    }
}
