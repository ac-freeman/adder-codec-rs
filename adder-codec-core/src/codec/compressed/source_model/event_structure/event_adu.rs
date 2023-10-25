use crate::codec::compressed::fenwick::context_switching::FenwickModel;
use crate::codec::compressed::source_model::cabac_contexts::Contexts;
use crate::codec::compressed::source_model::event_structure::event_cube::EventCube;
use crate::codec::compressed::source_model::event_structure::BLOCK_SIZE;
use crate::codec::compressed::source_model::{ComponentCompression, HandleEvent};
use crate::codec::CodecError;
use crate::{AbsoluteT, DeltaT, Event, PixelAddress, PlaneSize};
use arithmetic_coding::{Decoder, Encoder};
use bitstream_io::{BigEndian, BitReader, BitWriter};
use ndarray::Array2;
use std::io::Cursor;
use std::mem::size_of;

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
        let blocks_y = (plane.h_usize() + BLOCK_SIZE - 1) / BLOCK_SIZE;
        let blocks_x = (plane.w_usize() + BLOCK_SIZE - 1) / BLOCK_SIZE;

        Self {
            plane,
            event_cubes: Array2::from_shape_fn((blocks_y, blocks_x), |(y, x)| {
                EventCube::new(
                    y as u16 * BLOCK_SIZE as u16,
                    x as u16 * BLOCK_SIZE as u16,
                    plane.c_usize(),
                    start_t,
                    dt_ref,
                    num_intervals,
                )
            }),
            start_t,
            dt_ref,
            num_intervals,
            skip_adu: true,
            cube_to_write_count: 0,
        }
    }

    fn compress(
        &self,
        encoder: &mut Encoder<FenwickModel, BitWriter<Vec<u8>, BigEndian>>,
        contexts: &Contexts,
        stream: &mut BitWriter<Vec<u8>, BigEndian>,
    ) -> Result<(), CodecError> {
        // Write out the starting timestamp of the Adu
        encoder.model.set_context(contexts.t_context);
        for byte in self.start_t.to_be_bytes().iter() {
            encoder.encode(Some(&(*byte as usize)), stream).unwrap();
        }

        for cube in self.event_cubes.iter() {
            cube.compress(encoder, contexts, stream)?;
        }

        Ok(())
    }

    fn decompress(
        decoder: &mut Decoder<FenwickModel, BitReader<Cursor<Vec<u8>>, BigEndian>>,
        contexts: &Contexts,
        stream: &mut BitReader<Cursor<Vec<u8>>, BigEndian>,
        plane: PlaneSize,
        start_t: AbsoluteT,
        dt_ref: DeltaT,
        num_intervals: usize,
    ) -> Self {
        let mut adu = Self::new(plane, start_t, dt_ref, num_intervals);

        // Read the starting timestamp of the Adu
        decoder.model.set_context(contexts.t_context);
        let mut start_t = [0u8; size_of::<AbsoluteT>()];

        for byte in start_t.iter_mut() {
            *byte = decoder.decode(stream).unwrap().unwrap() as u8;
        }

        for block_idx_y in 0..adu.event_cubes.nrows() {
            for block_idx_x in 0..adu.event_cubes.ncols() {
                adu.event_cubes[[block_idx_y, block_idx_x]] = EventCube::decompress(
                    decoder,
                    contexts,
                    stream,
                    block_idx_y,
                    block_idx_x,
                    adu.plane.c_usize(),
                    adu.start_t,
                    dt_ref,
                    num_intervals,
                );
            }
        }

        adu
    }
}

impl HandleEvent for EventAdu {
    /// Take in a raw event and place it at the appropriate location.
    ///
    /// Assume that the event does fit within the adu's time frame. This is checked at the caller.
    ///
    /// Returns true if this is the first event that the Adu has ingested
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

    fn clear_compression(&mut self) {
        for cube in self.event_cubes.iter_mut() {
            cube.clear_compression();
        }
        self.skip_adu = true;
        self.cube_to_write_count = 0;
        self.start_t += self.num_intervals as AbsoluteT * self.dt_ref;
    }

    fn clear_decompression(&mut self) {
        for cube in self.event_cubes.iter_mut() {
            cube.clear_compression();
        }
        self.skip_adu = true;
        self.cube_to_write_count = 0;
        self.start_t += self.num_intervals as AbsoluteT * self.dt_ref;
    }
}

#[cfg(test)]
mod tests {
    use crate::codec::compressed::fenwick::context_switching::FenwickModel;
    use crate::codec::compressed::source_model::cabac_contexts::eof_context;
    use crate::codec::compressed::source_model::event_structure::event_adu::EventAdu;
    use crate::codec::compressed::source_model::event_structure::BLOCK_SIZE;
    use crate::codec::compressed::source_model::HandleEvent;
    use crate::codec::CodecMetadata;
    use crate::{AbsoluteT, Coord, DeltaT, Event, PlaneSize};
    use arithmetic_coding::Encoder;
    use bitstream_io::{BigEndian, BitReader, BitWriter};
    use ndarray::Array2;
    use std::cmp::min;
    use std::io::Cursor;

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

        assert_eq!(adu.event_cubes.shape(), &[2, 1]);

        Ok(())
    }

    #[test]
    fn compress_tiny_adu_intra() -> Result<(), Box<dyn std::error::Error>> {
        let plane = PlaneSize::new(16, 30, 1)?;
        let start_t = 0;
        let dt_ref = 255;
        let num_intervals = 10;

        let mut adu = EventAdu::new(plane, start_t, dt_ref, num_intervals);

        assert_eq!(adu.event_cubes.shape(), &[2, 1]);

        let mut counter = 0;
        for y in 0..30 {
            for x in 0..16 {
                adu.ingest_event(Event {
                    coord: Coord { x, y, c: None },
                    t: min(280 + counter, start_t + dt_ref * num_intervals as u32),
                    d: 7,
                });
                if 280 + counter > start_t + dt_ref * num_intervals as u32 {
                    break;
                } else {
                    counter += 1;
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

        adu.compress(&mut encoder, &contexts, &mut stream)?;
        eof_context(&contexts, &mut encoder, &mut stream);

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

        let adu2 = EventAdu::decompress(
            &mut decoder,
            &contexts,
            &mut stream,
            plane,
            start_t,
            dt_ref,
            num_intervals,
        );

        assert_eq!(adu.event_cubes.shape(), adu2.event_cubes.shape());
        let mut pixel_count = 0;
        for (cube1, cube2) in adu.event_cubes.iter().zip(adu2.event_cubes.iter()) {
            for (block1, block2) in cube1
                .raw_event_lists
                .iter()
                .zip(cube2.raw_event_lists.iter())
            {
                assert_eq!(block1.len(), block2.len());
                for (row1, row2) in block1.iter().zip(block2.iter()) {
                    for (px1, px2) in row1.iter().zip(row2.iter()) {
                        if px1.is_some() && px1.clone().unwrap().len() > 0 {
                            pixel_count += 1;
                            assert_eq!(px1, px2);
                        } else {
                            assert!(px1 == px2 || px2.is_none());
                        }
                    }
                }
            }
        }

        Ok(())
    }

    #[test]
    fn compress_tiny_adu_inter() -> Result<(), Box<dyn std::error::Error>> {
        let plane = PlaneSize::new(16, 30, 1)?;
        let start_t = 0;
        let dt_ref = 255;
        let num_intervals = 10;

        let mut adu = EventAdu::new(plane, start_t, dt_ref, num_intervals);

        assert_eq!(adu.event_cubes.shape(), &[2, 1]);

        let mut counter = 0;
        for y in 0..30 {
            for x in 0..16 {
                for _ in 0..3 {
                    adu.ingest_event(Event {
                        coord: Coord { x, y, c: None },
                        t: min(280 + counter, start_t + dt_ref * num_intervals as u32),
                        d: 7,
                    });
                    if 28 + counter > start_t + dt_ref * num_intervals as u32 {
                        break;
                    } else {
                        counter += 1;
                    }
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

        adu.compress(&mut encoder, &contexts, &mut stream)?;
        eof_context(&contexts, &mut encoder, &mut stream);

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
        let encoded_data = stream.into_writer();
        let mut stream = BitReader::endian(Cursor::new(encoded_data.clone()), BigEndian);

        let adu2 = EventAdu::decompress(
            &mut decoder,
            &contexts,
            &mut stream,
            plane,
            start_t,
            dt_ref,
            num_intervals,
        );

        assert_eq!(adu.event_cubes.shape(), adu2.event_cubes.shape());
        let mut pixel_count = 0;
        for (cube1, cube2) in adu.event_cubes.iter().zip(adu2.event_cubes.iter()) {
            for (block1, block2) in cube1
                .raw_event_lists
                .iter()
                .zip(cube2.raw_event_lists.iter())
            {
                assert_eq!(block1.len(), block2.len());
                for (row1, row2) in block1.iter().zip(block2.iter()) {
                    for (px1, px2) in row1.iter().zip(row2.iter()) {
                        if px1.is_some() && px1.clone().unwrap().len() > 0 {
                            pixel_count += 1;
                            assert_eq!(px1, px2);
                        } else {
                            assert!(px1 == px2 || px2.is_none());
                        }
                    }
                }
            }
        }

        dbg!(encoded_data.len());
        dbg!(pixel_count * 9);
        assert!(encoded_data.len() < pixel_count * 9);

        Ok(())
    }
}
