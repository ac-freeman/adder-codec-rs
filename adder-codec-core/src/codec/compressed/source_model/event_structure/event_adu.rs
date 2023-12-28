use crate::codec::compressed::fenwick::context_switching::FenwickModel;
use crate::codec::compressed::source_model::cabac_contexts::{eof_context, Contexts};
use crate::codec::compressed::source_model::event_structure::event_cube::EventCube;
use crate::codec::compressed::source_model::event_structure::BLOCK_SIZE;
use crate::codec::compressed::source_model::{ComponentCompression, HandleEvent};
use crate::codec::CodecError;
use crate::{AbsoluteT, DeltaT, Event, PlaneSize};
use arithmetic_coding_adder_dep::{Decoder, Encoder};
use bitstream_io::{BigEndian, BitReader, BitWriter};
use ndarray::Array2;
use std::io::Cursor;
use std::mem::size_of;

#[derive(Clone, Debug, Default)]
pub struct EventAdu {
    /// Contains the sparse events in the cube. The index is the relative interval of dt_ref from the start
    event_cubes: Array2<EventCube>,

    /// The absolute time of the Adu's beginning (not necessarily aligned to an event. We structure
    /// cubes to be in temporal lockstep at the beginning.)
    pub(crate) start_t: AbsoluteT,

    /// How many ticks each input interval spans
    pub(crate) dt_ref: DeltaT,

    /// How many dt_ref intervals the whole adu spans
    pub(crate) num_intervals: usize,

    skip_adu: bool,

    cube_to_write_count: u16,

    pub(crate) state: AduState,

    first_run: bool,

    decompress_block_idx: (usize, usize), // decompressed_event_queue: VecDeque<Event>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub enum AduState {
    Compressed,
    Decompressed,
    #[default]
    Empty,
}

impl EventAdu {
    pub(crate) fn new(
        plane: PlaneSize,
        start_t: AbsoluteT,
        dt_ref: DeltaT,
        num_intervals: usize,
    ) -> Self {
        let blocks_y = (plane.h_usize() + BLOCK_SIZE - 1) / BLOCK_SIZE;
        let blocks_x = (plane.w_usize() + BLOCK_SIZE - 1) / BLOCK_SIZE;

        Self {
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
            // decompressed_event_queue: VecDeque::with_capacity(plane.volume() * 4),
            state: Default::default(),
            first_run: true,
            decompress_block_idx: (0, 0),
        }
    }

    pub fn compress(
        &mut self,
        stream: &mut BitWriter<Vec<u8>, BigEndian>,
        c_thresh_max: u8,
    ) -> Result<(), CodecError> {
        // Create a new source model instance
        let mut source_model = FenwickModel::with_symbols(u16::MAX as usize, 1 << 30);
        let contexts = Contexts::new(&mut source_model, self.dt_ref);

        let mut encoder = Encoder::new(source_model);

        // Write out the starting timestamp of the Adu
        encoder.model.set_context(contexts.t_context);
        for byte in self.start_t.to_be_bytes().iter() {
            encoder.encode(Some(&(*byte as usize)), stream).unwrap();
        }

        for cube in self.event_cubes.iter_mut() {
            debug_assert_eq!(cube.start_t, self.start_t);
            cube.compress_intra(&mut encoder, &contexts, stream, Some(c_thresh_max))?;
        }

        for cube in self.event_cubes.iter_mut() {
            debug_assert_eq!(cube.start_t, self.start_t);
            cube.compress_inter(&mut encoder, &contexts, stream, Some(c_thresh_max))?;
        }

        // Flush the encoder
        eof_context(&contexts, &mut encoder, stream);

        self.clear_compression();

        Ok(())
    }

    pub fn decompress(&mut self, stream: &mut BitReader<Cursor<Vec<u8>>, BigEndian>) {
        self.clear_decompression();

        // let mut adu = Self::new(plane, start_t, dt_ref, num_intervals);

        // Create a new source model instance
        let mut source_model = FenwickModel::with_symbols(u16::MAX as usize, 1 << 30);
        let contexts = Contexts::new(&mut source_model, self.dt_ref);
        let mut decoder = Decoder::new(source_model);

        // Read the starting timestamp of the Adu
        decoder.model.set_context(contexts.t_context);
        let mut start_t = [0u8; size_of::<AbsoluteT>()];

        for byte in start_t.iter_mut() {
            *byte = decoder.decode(stream).unwrap().unwrap() as u8;
        }

        for block_idx_y in 0..self.event_cubes.nrows() {
            for block_idx_x in 0..self.event_cubes.ncols() {
                self.event_cubes[[block_idx_y, block_idx_x]].decompress_intra(
                    &mut decoder,
                    &contexts,
                    stream,
                    self.start_t,
                );
                debug_assert_eq!(
                    self.event_cubes[[block_idx_y, block_idx_x]].start_t,
                    self.start_t
                );
            }
        }

        for block_idx_y in 0..self.event_cubes.nrows() {
            for block_idx_x in 0..self.event_cubes.ncols() {
                self.event_cubes[[block_idx_y, block_idx_x]].decompress_inter(
                    &mut decoder,
                    &contexts,
                    stream,
                );
                debug_assert_eq!(
                    self.event_cubes[[block_idx_y, block_idx_x]].start_t,
                    self.start_t
                );
            }
        }
        self.state = AduState::Decompressed;
        self.first_run = false;
    }

    pub fn decoder_is_empty(&self) -> bool {
        self.state == AduState::Empty
    }
}

impl HandleEvent for EventAdu {
    /// Take in a raw event and place it at the appropriate location.
    ///
    /// Assume that the event does fit within the adu's time frame. This is checked at the caller.
    ///
    /// Returns true if this is the first event that the Adu has ingested
    fn ingest_event(&mut self, event: Event) -> bool {
        let idx_y = event.coord.y_usize() / BLOCK_SIZE;
        let idx_x = event.coord.x_usize() / BLOCK_SIZE;

        if self.event_cubes[[idx_y, idx_x]].ingest_event(event) {
            self.cube_to_write_count += 1;
        };

        if self.skip_adu {
            self.skip_adu = false;
            true
        } else {
            false
        }
    }

    fn digest_event(&mut self) -> Result<Event, CodecError> {
        let (a, b) = self.decompress_block_idx;
        match self.event_cubes[[a, b]].digest_event() {
            Err(CodecError::NoMoreEvents) => {
                if a == self.event_cubes.shape()[0] - 1 && b == self.event_cubes.shape()[1] - 1 {
                    self.state = AduState::Empty;
                    return Err(CodecError::NoMoreEvents);
                } else if b == self.event_cubes.shape()[1] - 1 {
                    self.decompress_block_idx = (a + 1, 0);
                } else {
                    self.decompress_block_idx = (a, b + 1);
                }

                // Call it recursively on the new block idx
                self.digest_event()
            }
            Ok(event) => Ok(event),
            Err(e) => Err(e),
        }
    }

    fn clear_compression(&mut self) {
        for cube in self.event_cubes.iter_mut() {
            cube.clear_compression();
        }
        self.skip_adu = true;
        self.cube_to_write_count = 0;
        self.start_t += self.num_intervals as AbsoluteT * self.dt_ref;
        self.state = AduState::Empty;
    }

    fn clear_decompression(&mut self) {
        if !(self.first_run) {
            // Only do this reset if we're not at the very beginning of the stream
            for cube in self.event_cubes.iter_mut() {
                cube.clear_compression();
            }
            self.skip_adu = true;
            self.cube_to_write_count = 0;
            self.start_t += self.num_intervals as AbsoluteT * self.dt_ref;
            self.decompress_block_idx = (0, 0);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::codec::compressed::fenwick::context_switching::FenwickModel;
    use crate::codec::compressed::source_model::cabac_contexts::{eof_context, Contexts};
    use crate::codec::compressed::source_model::event_structure::event_adu::EventAdu;
    use crate::codec::compressed::source_model::{ComponentCompression, HandleEvent};
    use crate::codec::CodecError;
    use crate::{Coord, Event, PlaneSize};
    use arithmetic_coding_adder_dep::Encoder;
    use bitstream_io::{BigEndian, BitReader, BitWriter};
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

        compress_test(&mut adu, &mut stream, 0)?;

        let mut stream = BitReader::endian(Cursor::new(stream.into_writer()), BigEndian);
        let mut adu2 = EventAdu::new(plane, start_t, dt_ref, num_intervals);
        adu2.decompress(&mut stream);

        assert_eq!(adu.event_cubes.shape(), adu2.event_cubes.shape());
        for (cube1, cube2) in adu.event_cubes.iter().zip(adu2.event_cubes.iter()) {
            for (block1, block2) in cube1
                .raw_event_lists
                .iter()
                .zip(cube2.raw_event_lists.iter())
            {
                assert_eq!(block1.len(), block2.len());
                for (row1, row2) in block1.iter().zip(block2.iter()) {
                    for (px1, px2) in row1.iter().zip(row2.iter()) {
                        if !px1.is_empty() {
                            for (elem1, elem2) in px1.iter().zip(px2.iter()) {
                                assert!(elem1.t == elem2.t || px2.is_empty());
                            }
                        } else {
                            assert!(px1 == px2 || px2.is_empty());
                        }
                    }
                }
            }
        }

        Ok(())
    }

    fn compress_test(
        adu: &mut EventAdu,
        stream: &mut BitWriter<Vec<u8>, BigEndian>,
        c_thresh_max: u8,
    ) -> Result<(), CodecError> {
        // Create a new source model instance
        let mut source_model = FenwickModel::with_symbols(u16::MAX as usize, 1 << 30);
        let contexts = Contexts::new(&mut source_model, adu.dt_ref);

        let mut encoder = Encoder::new(source_model);

        // Write out the starting timestamp of the Adu
        encoder.model.set_context(contexts.t_context);
        for byte in adu.start_t.to_be_bytes().iter() {
            encoder.encode(Some(&(*byte as usize)), stream).unwrap();
        }

        for cube in adu.event_cubes.iter_mut() {
            debug_assert_eq!(cube.start_t, adu.start_t);
            cube.compress_intra(&mut encoder, &contexts, stream, Some(c_thresh_max))?;
        }

        for cube in adu.event_cubes.iter_mut() {
            debug_assert_eq!(cube.start_t, adu.start_t);
            cube.compress_inter(&mut encoder, &contexts, stream, Some(c_thresh_max))?;
        }

        // Flush the encoder
        eof_context(&contexts, &mut encoder, stream);

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
                    let event = Event {
                        coord: Coord { x, y, c: None },
                        t: min(280 + counter, start_t + dt_ref * num_intervals as u32),
                        d: 7,
                    };
                    counter += 1;
                    if counter == 233 {
                        dbg!(event);
                    }
                    adu.ingest_event(event);

                    if 28 + counter > start_t + dt_ref * num_intervals as u32 {
                        break;
                    }
                }
            }
        }

        let bufwriter = Vec::new();
        let mut stream = BitWriter::endian(bufwriter, BigEndian);

        compress_test(&mut adu, &mut stream, 0)?;

        let encoded_data = stream.into_writer();
        let mut stream = BitReader::endian(Cursor::new(encoded_data.clone()), BigEndian);
        let mut adu2 = EventAdu::new(plane, start_t, dt_ref, num_intervals);
        adu2.decompress(&mut stream);

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
                        if !px1.is_empty() {
                            for (elem1, elem2) in px1.iter().zip(px2.iter()) {
                                pixel_count += 1;
                                assert!(elem1.t == elem2.t || px2.is_empty());
                            }
                        } else {
                            assert!(px1 == px2 || px2.is_empty());
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
