use crate::codec::compressed::fenwick::context_switching::FenwickModel;
use crate::codec::compressed::source_model::cabac_contexts::Contexts;
use crate::codec::compressed::source_model::event_structure::{BLOCK_SIZE, BLOCK_SIZE_AREA};
use crate::codec::compressed::source_model::{ComponentCompression, HandleEvent};
use crate::codec::compressed::{DResidual, TResidual, DRESIDUAL_NO_EVENT};
use crate::codec::CodecError;
use crate::{AbsoluteT, DeltaT, Event, EventCoordless, PixelAddress, D, D_NO_EVENT};
use arithmetic_coding::{Decoder, Encoder};
use bitstream_io::{BigEndian, BitReader, BitWriter};
use std::cmp::min;
use std::collections::HashMap;
use std::io::Cursor;
use std::mem::size_of;

type Pixel = Option<Vec<(u8, EventCoordless)>>;

#[derive(PartialEq, Debug)]
pub struct EventCube {
    /// The absolute y-coordinate of the top-left pixel in the cube
    pub(crate) start_y: PixelAddress,

    /// The absolute x-coordinate of the top-left pixel in the cube
    pub(crate) start_x: PixelAddress,

    num_channels: usize,

    /// Contains the sparse events in the cube. The index is the relative interval of dt_ref from the start
    raw_event_lists: [[[Pixel; BLOCK_SIZE]; BLOCK_SIZE]; 3],

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
        num_channels: usize,
        start_t: AbsoluteT,
        dt_ref: DeltaT,
        num_intervals: usize,
    ) -> Self {
        let row: [Pixel; BLOCK_SIZE] = vec![Some(Vec::with_capacity(num_intervals)); BLOCK_SIZE]
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
        }
    }

    fn compress_intra(
        &self,
        encoder: &mut Encoder<FenwickModel, BitWriter<Vec<u8>, BigEndian>>,
        contexts: &Contexts,
        stream: &mut BitWriter<Vec<u8>, BigEndian>,
    ) -> Result<(), CodecError> {
        let mut init_event: Option<EventCoordless> = None;

        // Intra-code the first event (if present) for each pixel in row-major order
        self.raw_event_lists.iter().for_each(|channel| {
            channel.iter().for_each(|row| {
                row.iter().for_each(|pixel_opt| {
                    encoder.model.set_context(contexts.d_context);

                    if pixel_opt.is_some() && !pixel_opt.as_ref().unwrap().is_empty() {
                        let event = pixel_opt.as_ref().unwrap().first().unwrap().1;

                        let mut d_residual = 0;

                        if let Some(init) = &mut init_event {
                            d_residual = event.d as DResidual - init.d as DResidual;
                            // Write the D residual (relative to the start_d for the first event)
                            for byte in d_residual.to_be_bytes().iter() {
                                encoder.encode(Some(&(*byte as usize)), stream).unwrap();
                            }
                        } else {
                            // Write the first event's D directly
                            for byte in (event.d as DResidual).to_be_bytes().iter() {
                                encoder.encode(Some(&(*byte as usize)), stream).unwrap();
                            }

                            // Create the init event with t being the start_t of the cube
                            init_event = Some(EventCoordless {
                                d: event.d,
                                t: self.start_t,
                            })
                        }

                        if let Some(init) = &mut init_event {
                            encoder.model.set_context(contexts.dtref_context);

                            // Don't do any special prediction here (yet). Just predict the same t as previously found.
                            let mut t_residual = (event.t as i32 - init.t as i32) as TResidual;
                            let mut dtref_residual = t_residual / self.dt_ref as TResidual;
                            for byte in dtref_residual.to_be_bytes().iter() {
                                encoder.encode(Some(&(*byte as usize)), stream).unwrap();
                            }
                            t_residual = t_residual % self.dt_ref as TResidual; // TODO: check this math

                            encoder.model.set_context(contexts.t_context);

                            for byte in t_residual.to_be_bytes().iter() {
                                encoder.encode(Some(&(*byte as usize)), stream).unwrap();
                            }
                            *init = event;
                        } else {
                            panic!("No init event");
                        }
                    } else {
                        // Else there's no event for this pixel. Encode a NO_EVENT symbol.
                        for byte in (DRESIDUAL_NO_EVENT).to_be_bytes().iter() {
                            encoder.encode(Some(&(*byte as usize)), stream).unwrap();
                        }
                    }
                })
            })
        });
        Ok(())
    }

    fn decompress_intra(
        decoder: &mut Decoder<FenwickModel, BitReader<Cursor<Vec<u8>>, BigEndian>>,
        contexts: &Contexts,
        stream: &mut BitReader<Cursor<Vec<u8>>, BigEndian>,
        block_idx_y: usize,
        block_idx_x: usize,
        num_channels: usize,
        start_t: AbsoluteT,
        dt_ref: DeltaT,
        num_intervals: usize,
    ) -> Self {
        let mut cube = Self::new(
            block_idx_y as PixelAddress * BLOCK_SIZE as u16,
            block_idx_x as PixelAddress * BLOCK_SIZE as u16,
            num_channels,
            start_t,
            dt_ref,
            num_intervals,
        );

        let mut d_residual_buffer = [0u8; size_of::<DResidual>()];
        let mut dtref_residual_buffer = [0u8; size_of::<TResidual>()];
        let mut t_residual_buffer = [0u8; size_of::<TResidual>()];
        let mut init_event: Option<EventCoordless> = None;

        cube.raw_event_lists.iter_mut().for_each(|channel| {
            channel.iter_mut().for_each(|row| {
                row.iter_mut().for_each(|pixel| {
                    decoder.model.set_context(contexts.d_context);

                    for byte in d_residual_buffer.iter_mut() {
                        *byte = decoder.decode(stream).unwrap().unwrap() as u8;
                    }
                    let d_residual = DResidual::from_be_bytes(d_residual_buffer);

                    if d_residual == DRESIDUAL_NO_EVENT {
                        *pixel = None; // So we can skip it for inter-coding
                    } else {
                        let d = if let Some(init) = &mut init_event {
                            (init.d as DResidual + d_residual) as D
                        } else {
                            // There is no init event
                            init_event = Some(EventCoordless { d: 0, t: start_t });
                            d_residual as D
                        };

                        if let Some(init) = &mut init_event {
                            decoder.model.set_context(contexts.dtref_context);
                            for byte in dtref_residual_buffer.iter_mut() {
                                *byte = decoder.decode(stream).unwrap().unwrap() as u8;
                            }
                            let dtref_residual = DResidual::from_be_bytes(dtref_residual_buffer);

                            decoder.model.set_context(contexts.t_context);
                            for byte in t_residual_buffer.iter_mut() {
                                *byte = decoder.decode(stream).unwrap().unwrap() as u8;
                            }
                            let mut t_residual = DResidual::from_be_bytes(t_residual_buffer);

                            t_residual += dtref_residual * dt_ref as DResidual;

                            init.d = (init.d as DResidual + d_residual) as D;
                            init.t = (init.t as TResidual + t_residual) as AbsoluteT;
                            pixel.as_mut().unwrap().push((
                                ((init.t - start_t) / dt_ref) as u8,
                                EventCoordless { d, t: init.t },
                            ));
                        } else {
                            panic!("No init event");
                        }
                    }
                });
            });
        });

        cube
    }

    fn compress_inter(
        &self,
        encoder: &mut Encoder<FenwickModel, BitWriter<Vec<u8>, BigEndian>>,
        contexts: &Contexts,
        stream: &mut BitWriter<Vec<u8>, BigEndian>,
    ) -> Result<(), CodecError> {
        let mut init_event: Option<EventCoordless> = None;

        // Intra-code the first event (if present) for each pixel in row-major order
        self.raw_event_lists.iter().for_each(|channel| {
            channel.iter().for_each(|row| {
                row.iter().for_each(|pixel_opt| {
                    if pixel_opt.is_some() && !pixel_opt.as_ref().unwrap().is_empty() {
                        let pixel = pixel_opt.as_ref().unwrap();

                        let mut idx = 1;
                        let mut last_delta_t: DeltaT = 0;
                        loop {
                            let mut prev_event = pixel[idx - 1]; // We can assume for now that this is perfectly decoded, but later we'll corrupt it according to any loss we incur

                            encoder.model.set_context(contexts.d_context);

                            if idx < pixel.len() {
                                let event = pixel[idx];

                                // Get the D residual
                                let d_residual =
                                    event.1.d as DResidual - prev_event.1.d as DResidual;
                                // Write the D residual (relative to the start_d for the first event)
                                for byte in d_residual.to_be_bytes().iter() {
                                    encoder.encode(Some(&(*byte as usize)), stream).unwrap();
                                }

                                let t_prediction = if idx == 1 {
                                    // We don't have a deltaT context, so just predict double the dtref of the previous event
                                    min(prev_event.0 * 2, self.num_intervals as u8 - 1) as AbsoluteT
                                        + self.start_t
                                } else {
                                    // We've gotten the DeltaT between the last two events. Use that
                                    // to form our prediction
                                    let delta_t_prediction: DeltaT = if d_residual < 0 {
                                        last_delta_t >> -d_residual
                                    } else {
                                        last_delta_t << d_residual
                                    };
                                    prev_event.1.t
                                        + min(
                                            delta_t_prediction,
                                            ((self.num_intervals as u8 - event.0) as u32
                                                * self.dt_ref),
                                        ) as AbsoluteT
                                };

                                last_delta_t = event.1.t - prev_event.1.t;

                                encoder.model.set_context(contexts.dtref_context);
                                let mut t_residual =
                                    (event.1.t as i32 - t_prediction as i32) as TResidual;
                                let mut dtref_residual = t_residual / self.dt_ref as TResidual;
                                for byte in dtref_residual.to_be_bytes().iter() {
                                    encoder.encode(Some(&(*byte as usize)), stream).unwrap();
                                }
                                t_residual = t_residual % self.dt_ref as TResidual; // TODO: check this math

                                encoder.model.set_context(contexts.t_context);

                                for byte in t_residual.to_be_bytes().iter() {
                                    encoder.encode(Some(&(*byte as usize)), stream).unwrap();
                                }
                            } else {
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
        });
        Ok(())
    }

    fn decompress_inter(
        &mut self,
        decoder: &mut Decoder<FenwickModel, BitReader<Cursor<Vec<u8>>, BigEndian>>,
        contexts: &Contexts,
        stream: &mut BitReader<Cursor<Vec<u8>>, BigEndian>,
    ) -> Self {
    }
}

impl HandleEvent for EventCube {
    /// Take in a raw event and place it at the appropriate location.
    ///
    /// Assume that the event does fit within the cube's time frame. This is checked at the caller.
    fn ingest_event(&mut self, mut event: Event) -> bool {
        event.coord.y -= self.start_y;
        event.coord.x -= self.start_x;

        let index = if event.t < self.start_t {
            0
        } else {
            ((event.t - self.start_t) / self.dt_ref) as u8
        };

        let item = (
            index, // The index: the relative interval of dt_ref from the start
            EventCoordless::from(event),
        );
        if let Some(ref mut list) = &mut self.raw_event_lists[event.coord.c_usize()]
            [event.coord.y_usize()][event.coord.x_usize()]
        {
            list.push(item);
        } else {
            self.raw_event_lists[event.coord.c_usize()][event.coord.y_usize()]
                [event.coord.x_usize()] = Some(vec![item]);
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

    fn digest_event(&mut self) {
        todo!()
    }

    /// Clear out the cube's events and increment the start time by the cube's duration
    fn clear_compression(&mut self) {
        for c in 0..3 {
            for y in 0..BLOCK_SIZE {
                for x in 0..BLOCK_SIZE {
                    if let Some(ref mut pixel) = &mut self.raw_event_lists[c][y][x] {
                        pixel.clear();
                    }
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
                    self.raw_event_lists[c][y][x] = None;
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
    use crate::{Coord, Event, PixelAddress};

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
        assert!(cube.raw_event_lists[0][0][0].as_ref().unwrap().is_empty());
        assert_eq!(cube.raw_event_lists[0][1][13].as_ref().unwrap().len(), 1);

        Ok(())
    }

    #[test]
    fn fill_second_cube() -> Result<(), Box<dyn std::error::Error>> {
        let mut cube = fill_cube()?;
        cube.clear_compression();
        assert_eq!(cube.raw_event_lists[0][1][13].as_ref().unwrap().len(), 0);
        cube.ingest_event(Event {
            coord: Coord {
                x: 29,
                y: 17,
                c: None,
            },
            t: 500,
            d: 7,
        });
        assert_eq!(cube.raw_event_lists[0][1][13].as_ref().unwrap().len(), 1);
        Ok(())
    }
}

impl ComponentCompression for EventCube {
    fn compress(
        &self,
        encoder: &mut Encoder<FenwickModel, BitWriter<Vec<u8>, BigEndian>>,
        contexts: &Contexts,
        stream: &mut BitWriter<Vec<u8>, BigEndian>,
    ) -> Result<(), CodecError> {
        self.compress_intra(encoder, contexts, stream)?;
        self.compress_inter(encoder, contexts, stream)?;
        Ok(())
    }

    fn decompress(
        decoder: &mut Decoder<FenwickModel, BitReader<Cursor<Vec<u8>>, BigEndian>>,
        contexts: &Contexts,
        stream: &mut BitReader<Cursor<Vec<u8>>, BigEndian>,
        block_idx_y: usize,
        block_idx_x: usize,
        num_channels: usize,
        start_t: AbsoluteT,
        dt_ref: DeltaT,
        num_intervals: usize,
    ) -> Self {
        let mut cube = EventCube::decompress_intra(
            decoder,
            contexts,
            stream,
            block_idx_y,
            block_idx_x,
            num_channels,
            start_t,
            dt_ref,
            num_intervals,
        );
        cube.decompress_inter(decoder, contexts, stream)
    }
}

#[cfg(test)]
mod compression_tests {
    use crate::codec::compressed::fenwick::context_switching::FenwickModel;
    use crate::codec::compressed::source_model::event_structure::event_cube::EventCube;
    use crate::codec::compressed::source_model::{ComponentCompression, HandleEvent};
    use crate::codec::CodecMetadata;
    use crate::{Coord, Event};
    use arithmetic_coding::Encoder;
    use bitstream_io::{BigEndian, BitReader, BitWriter};
    use rand::prelude::StdRng;
    use rand::{Rng, SeedableRng};
    use std::cmp::min;
    use std::error::Error;
    use std::io::Cursor;

    #[test]
    fn compress_and_decompress_intra() -> Result<(), Box<dyn Error>> {
        let mut cube = EventCube::new(0, 0, 1, 255, 255, 2550);
        let mut counter = 0;
        for c in 0..3 {
            for y in 0..16 {
                for x in 0..15 {
                    cube.ingest_event(Event {
                        coord: Coord { x, y, c: None },
                        t: 280 + counter,
                        d: 7,
                    });
                    counter += 10;
                }
            }
        }

        let bufwriter = Vec::new();
        let mut stream = BitWriter::endian(bufwriter, BigEndian);

        let mut source_model = FenwickModel::with_symbols(u16::MAX as usize, 1 << 30);
        let contexts = crate::codec::compressed::source_model::cabac_contexts::Contexts::new(
            &mut source_model,
            CodecMetadata {
                codec_version: 0,
                header_size: 0,
                time_mode: Default::default(),
                plane: Default::default(),
                tps: 0,
                ref_interval: 255,
                delta_t_max: 2550,
                event_size: 0,
                source_camera: Default::default(),
            },
        );

        let mut encoder = Encoder::new(source_model);

        cube.compress(&mut encoder, &contexts, &mut stream)?;

        let mut source_model = FenwickModel::with_symbols(u16::MAX as usize, 1 << 30);
        let contexts = crate::codec::compressed::source_model::cabac_contexts::Contexts::new(
            &mut source_model,
            CodecMetadata {
                codec_version: 0,
                header_size: 0,
                time_mode: Default::default(),
                plane: Default::default(),
                tps: 0,
                ref_interval: 255,
                delta_t_max: 2550,
                event_size: 0,
                source_camera: Default::default(),
            },
        );
        let mut decoder = arithmetic_coding::Decoder::new(source_model);
        let mut stream = BitReader::endian(Cursor::new(stream.into_writer()), BigEndian);

        let cube2 =
            EventCube::decompress(&mut decoder, &contexts, &mut stream, 0, 0, 1, 255, 255, 10);

        for c in 0..3 {
            for y in 0..16 {
                for x in 0..16 {
                    dbg!(c, y, x);
                    if let Some(ref pixel) = cube.raw_event_lists[c][y][x] {
                        if !pixel.is_empty() {
                            assert!(cube2.raw_event_lists[c][y][x].is_some());
                            assert_eq!(
                                cube.raw_event_lists[c][y][x].as_ref().unwrap()[0],
                                cube2.raw_event_lists[c][y][x].as_ref().unwrap()[0]
                            );
                        }
                    } else {
                        assert!(cube2.raw_event_lists[c][y][x].is_none());
                    }
                }
            }
        }

        Ok(())
    }

    #[test]
    fn compress_and_decompress_inter() -> Result<(), Box<dyn Error>> {
        let mut cube = EventCube::new(0, 0, 1, 255, 255, 10);
        let mut counter = 0;
        for c in 0..3 {
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

        let mut rng = StdRng::seed_from_u64(1234);

        for y in 0..16 {
            for x in 0..15 {
                for i in 0..rng.gen_range(0..3) {
                    cube.ingest_event(Event {
                        coord: Coord { x, y, c: None },
                        t: min(
                            280 + counter,
                            cube.start_t + (cube.num_intervals as u32 - 1) * cube.dt_ref,
                        ),
                        d: rng.gen_range(4..12),
                    });
                    counter += 1;
                    assert!(
                        cube.raw_event_lists[0][y as usize][x as usize]
                            .as_ref()
                            .unwrap()
                            .last()
                            .unwrap()
                            .0
                            < cube.num_intervals as u8
                    );
                }
            }
        }

        let bufwriter = Vec::new();
        let mut stream = BitWriter::endian(bufwriter, BigEndian);

        let mut source_model = FenwickModel::with_symbols(u16::MAX as usize, 1 << 30);
        let contexts = crate::codec::compressed::source_model::cabac_contexts::Contexts::new(
            &mut source_model,
            CodecMetadata {
                codec_version: 0,
                header_size: 0,
                time_mode: Default::default(),
                plane: Default::default(),
                tps: 0,
                ref_interval: 255,
                delta_t_max: 2550,
                event_size: 0,
                source_camera: Default::default(),
            },
        );

        let mut encoder = Encoder::new(source_model);

        cube.compress(&mut encoder, &contexts, &mut stream)?;

        let mut source_model = FenwickModel::with_symbols(u16::MAX as usize, 1 << 30);
        let contexts = crate::codec::compressed::source_model::cabac_contexts::Contexts::new(
            &mut source_model,
            CodecMetadata {
                codec_version: 0,
                header_size: 0,
                time_mode: Default::default(),
                plane: Default::default(),
                tps: 0,
                ref_interval: 255,
                delta_t_max: 2550,
                event_size: 0,
                source_camera: Default::default(),
            },
        );
        let mut decoder = arithmetic_coding::Decoder::new(source_model);
        let mut stream = BitReader::endian(Cursor::new(stream.into_writer()), BigEndian);

        let cube2 =
            EventCube::decompress(&mut decoder, &contexts, &mut stream, 0, 0, 1, 255, 255, 10);

        for c in 0..3 {
            for y in 0..16 {
                for x in 0..16 {
                    dbg!(c, y, x);
                    if let Some(ref pixel) = cube.raw_event_lists[c][y][x] {
                        if !pixel.is_empty() {
                            assert!(cube2.raw_event_lists[c][y][x].is_some());
                            assert_eq!(
                                cube.raw_event_lists[c][y][x].as_ref().unwrap()[0],
                                cube2.raw_event_lists[c][y][x].as_ref().unwrap()[0]
                            );
                        }
                    } else {
                        assert!(cube2.raw_event_lists[c][y][x].is_none());
                    }
                }
            }
        }

        Ok(())
    }
}
