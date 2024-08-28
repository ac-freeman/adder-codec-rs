use crate::codec::{CodecError, CodecMetadata, EncoderOptions, ReadCompression, WriteCompression};
use bitstream_io::{BigEndian, BitRead, BitReader, BitWrite, BitWriter};
use priority_queue::PriorityQueue;
use std::cmp::Reverse;
use std::io::{Cursor, Read, Seek, SeekFrom, Write};
use std::ops::{Add, AddAssign};
use std::sync::{Arc, RwLock};

use crate::codec::compressed::source_model::event_structure::event_adu::EventAdu;
use crate::codec::compressed::source_model::HandleEvent;
use crate::codec::header::{Magic, MAGIC_COMPRESSED};
use crate::codec::rate_controller::CrfParameters;
use crate::{DeltaT, Event};

/// A message to send to the writer thread (that is, the main thread) to write out the compressed
/// ADΔER data to the stream
pub(crate) struct BytesMessage {
    message_id: u32,
    bytes: Vec<u8>,
}

/// Write compressed ADΔER data to a stream.
pub struct CompressedOutput<W: Write> {
    pub(crate) meta: CodecMetadata,
    pub(crate) adu: EventAdu,
    pub(crate) stream: Option<Arc<RwLock<BitWriter<W, BigEndian>>>>,
    pub(crate) options: EncoderOptions,
    // pub(crate) written_bytes_rx: std::sync::mpsc::Receiver<BytesMessage>,
    pub(crate) written_bytes_tx: Option<std::sync::mpsc::Sender<BytesMessage>>,
    // pub(crate) bytes_writer_queue: PriorityQueue<Vec<u8>, Reverse<u32>>,
    /// The ID of the last message sent from a spawned compressor thread
    pub(crate) last_message_sent: u32,

    /// The ID of the last message received in the writer thread and actually written out the stream
    pub(crate) last_message_written: Arc<RwLock<u32>>,

    pub(crate) _phantom: std::marker::PhantomData<W>,
}

/// Read compressed ADΔER data from a stream.
pub struct CompressedInput<R: Read> {
    pub(crate) meta: CodecMetadata,

    adu: Option<EventAdu>,

    _phantom: std::marker::PhantomData<R>,
}

// fn compressor_worker<W: Write>(
//     mut stream: Arc<RwLock<BitWriter<W, BigEndian>>>,
//     rx: std::sync::mpsc::Receiver<(EventAdu, CrfParameters)>,
// ) -> Option<W> {
//     while let Ok((mut adu, parameters)) = rx.recv() {
//         let mut temp_stream = BitWriter::endian(Vec::new(), BigEndian);
//
//         // Compress the Adu. This also writes the EOF symbol and flushes the encoder
//         adu.compress(&mut temp_stream, parameters.c_thresh_max).ok();
//         let written_data = temp_stream.into_writer();
//
//         let mut stream_write = stream.write().unwrap();
//         // Write the number of bytes in the compressed Adu as the 32-bit header for this Adu
//         stream_write
//             .write_bytes(&(written_data.len() as u32).to_be_bytes())
//             .ok()?;
//
//         // Write the temporary stream to the actual stream
//         stream_write.write_bytes(&written_data).ok()?;
//     }
//
//     None
// }

fn flush_bytes_queue_worker<W: Write>(
    mut stream: Arc<RwLock<BitWriter<W, BigEndian>>>,
    written_bytes_rx: std::sync::mpsc::Receiver<BytesMessage>,
    last_message_written: Arc<RwLock<u32>>,
    mut bytes_writer_queue: PriorityQueue<Vec<u8>, Reverse<u32>>,
) {
    while let Ok(bytes_message) = written_bytes_rx.recv() {
        // Blocking recv
        // eprintln!("received message");

        bytes_writer_queue.push(bytes_message.bytes, Reverse(bytes_message.message_id));

        let mut last_message_written = last_message_written.write().unwrap();
        while let Some((bytes, message_id)) = bytes_writer_queue.pop() {
            if message_id == Reverse(*last_message_written + 1) {
                let mut stream_write = stream.write().unwrap();

                // Write the number of bytes in the compressed Adu as the 32-bit header for this Adu
                stream_write
                    .write_bytes(&(bytes.len() as u32).to_be_bytes())
                    .unwrap();
                stream_write.write_bytes(&bytes).unwrap();
                *last_message_written += 1;
            } else {
                bytes_writer_queue.push(bytes, message_id); // message_id here is already Reversed
                break;
            }
        }
    }
    eprintln!("Exiting writer thread...");
}

impl<W: Write + std::marker::Send + std::marker::Sync + 'static> CompressedOutput<W> {
    /// Create a new compressed output stream.
    pub fn new(meta: CodecMetadata, writer: W) -> Self {
        let adu = EventAdu::new(meta.plane, 0, meta.ref_interval, meta.adu_interval as usize);
        let (written_bytes_tx, written_bytes_rx) = std::sync::mpsc::channel();

        let stream_lock = RwLock::new(BitWriter::endian(writer, BigEndian));
        let stream_lock_arc = Arc::new(stream_lock);
        let stream_lock_arc_clone = stream_lock_arc.clone();

        let last_message_written = Arc::new(RwLock::new(0));
        let last_message_written_clone = last_message_written.clone();

        std::thread::spawn(move || {
            flush_bytes_queue_worker(
                stream_lock_arc_clone,
                written_bytes_rx,
                last_message_written_clone,
                PriorityQueue::new(),
            );
            eprintln!("Exiting writer thread...");
        });
        Self {
            meta,
            adu,
            // arithmetic_coder: Some(arithmetic_coder),
            // contexts: Some(contexts),
            stream: Some(stream_lock_arc),
            options: EncoderOptions::default(meta.plane),
            // written_bytes_rx,
            written_bytes_tx: Some(written_bytes_tx),
            // bytes_writer_queue: PriorityQueue::new(),
            last_message_sent: 0,
            last_message_written,
            _phantom: Default::default(),
        }
    }

    /// Keep the compressed encoder's option state synchronized with the high-level encoder container
    pub(crate) fn with_options(&mut self, options: EncoderOptions) {
        self.options = options;
    }

    /// Convenience function to get a mutable reference to the underlying stream.
    #[inline(always)]
    pub(crate) fn stream(&mut self) -> &mut Arc<RwLock<BitWriter<W, BigEndian>>> {
        self.stream.as_mut().unwrap()
    }
}

impl<W: Write + std::marker::Send + std::marker::Sync + 'static + 'static + 'static>
    WriteCompression<W> for CompressedOutput<W>
{
    fn magic(&self) -> Magic {
        MAGIC_COMPRESSED
    }

    fn meta(&self) -> &CodecMetadata {
        &self.meta
    }

    fn meta_mut(&mut self) -> &mut CodecMetadata {
        &mut self.meta
    }

    fn write_bytes(&mut self, bytes: &[u8]) -> Result<(), std::io::Error> {
        self.stream().write().unwrap().write_bytes(bytes)
    }

    fn byte_align(&mut self) -> std::io::Result<()> {
        self.stream().write().unwrap().byte_align()
    }

    fn into_writer(&mut self) -> Option<W> {
        if !self.adu.skip_adu {
            // while self.last_message_sent
            //     != self.last_message_written + self.bytes_writer_queue.len() as u32
            // {
            //     self.flush_bytes_queue();
            //
            //     // Sleep 1 second
            //     std::thread::sleep(std::time::Duration::from_secs(1));
            // }
            //
            // if let Some(stream) = &mut self.stream {
            //     while let Some((bytes, message_id)) = self.bytes_writer_queue.pop() {
            //         if message_id == Reverse(self.last_message_written + 1) {
            //             // Write the number of bytes in the compressed Adu as the 32-bit header for this Adu
            //             stream
            //                 .write_bytes(&(bytes.len() as u32).to_be_bytes())
            //                 .unwrap();
            //             stream.write_bytes(&bytes).unwrap();
            //             self.last_message_written += 1;
            //         } else {
            //             break;
            //         }
            //     }

            dbg!("compressing partial last adu");
            let mut temp_stream = BitWriter::endian(Vec::new(), BigEndian);

            let parameters = self.options.crf.get_parameters().clone();
            let mut adu = self.adu.clone();
            let tx = self.written_bytes_tx.as_ref().unwrap().clone();
            // Spawn a thread to compress the ADU and write out the data

            let message_id_to_send = self.last_message_sent + 1;
            self.last_message_sent += 1;

            std::thread::spawn(move || {
                adu.compress(&mut temp_stream, parameters.c_thresh_max).ok();
                let written_data = temp_stream.into_writer();

                tx.send(BytesMessage {
                    message_id: message_id_to_send,
                    bytes: written_data,
                })
                .unwrap();
            });
            // }
        }

        // Wait for the partial ADU to be written...
        while self.last_message_sent != *self.last_message_written.read().unwrap() {
            // Sleep 1 second
            eprintln!(
                "Sleeping. {} messages sent, {} messages written",
                self.last_message_sent,
                *self.last_message_written.read().unwrap()
            );
            std::thread::sleep(std::time::Duration::from_secs(1));
        }

        dbg!("All ADUs written.");
        // Kill the written_bytes_tx, so that the Arc only has one reference
        self.written_bytes_tx = None; // This will cause the flush_bytes_queue_worker() thread to
                                      // error out from the receiver, because the communication channel is severed

        std::thread::sleep(std::time::Duration::from_secs(1)); // Wait for the thread to exit. TODO: Make this deterministic, wait on the thread handle
        let arc = self.stream.take()?;

        let lock = Arc::into_inner(arc).unwrap();
        // let mut guard = tmp.write().unwrap();
        let consumed_data = lock.into_inner().unwrap();
        // let new_writer = BitWriter::endian(Default::default(), BigEndian);
        // let old_writer = std::mem::replace(&mut *guard, new_writer);
        Some(consumed_data.into_writer())
        //     let temp_writer = BitWriter::endian(W, BigEndian);
        //     let aa = std::mem::replace(&mut tmpp, temp_writer);
        //     tmpp.into_writer()
        // })
    }

    // fn into_writer(self: Self) -> Option<Box<W>> {
    //     Some(Box::new(self.stream.into_writer()))
    // }

    fn flush_writer(&mut self) -> std::io::Result<()> {
        self.stream().write().unwrap().flush()
    }

    fn ingest_event(&mut self, event: Event) -> Result<(), CodecError> {
        // Check that the event fits within the Adu's time range
        if event.t > self.adu.start_t + (self.adu.dt_ref * self.adu.num_intervals as DeltaT) {
            // dbg!("compressing adu");
            // If it doesn't, compress the events and reset the Adu

            // self.flush_bytes_queue();
            if self.stream.is_some() {
                // if let Some((bytes, message_id)) = self.bytes_writer_queue.pop() {
                //     if message_id == Reverse(self.last_message_written + 1) {
                //         // Write the number of bytes in the compressed Adu as the 32-bit header for this Adu
                //         stream
                //             .write_bytes(&(bytes.len() as u32).to_be_bytes())
                //             .unwrap();
                //         stream.write_bytes(&bytes).unwrap();
                //         self.last_message_written += 1;
                //     } else {
                //         self.bytes_writer_queue.push(bytes, message_id); // message_id here is already Reversed
                //                                                          // break;
                //     }
                // }

                // Create a temporary u8 stream to write the arithmetic-coded data to
                let mut temp_stream = BitWriter::endian(Vec::new(), BigEndian);

                let parameters = self.options.crf.get_parameters().clone();

                // Compress the Adu. This also writes the EOF symbol and flushes the encoder
                // First, clone the ADU
                let mut adu = self.adu.clone();
                let tx = self.written_bytes_tx.as_ref().unwrap().clone();
                // Spawn a thread to compress the ADU and write out the data

                let message_id_to_send = self.last_message_sent + 1;
                self.last_message_sent += 1;

                std::thread::spawn(move || {
                    adu.compress(&mut temp_stream, parameters.c_thresh_max).ok();
                    let written_data = temp_stream.into_writer();

                    tx.send(BytesMessage {
                        message_id: message_id_to_send,
                        bytes: written_data,
                    })
                    .unwrap();
                });

                self.adu.clear_compression();
            }
        }

        // Ingest the event in the Adu
        let _ = self.adu.ingest_event(event);

        Ok(())
    }
    // fn ingest_event_debug(&mut self, event: Event) -> Result<Option<Adu>, CodecError> {
    //     if let (true, _) = self.frame.add_event(event, self.meta.delta_t_max)? {
    //         let adu = self.compress_events()?;
    //         self.frame.add_event(event, self.meta.delta_t_max)?;
    //         return Ok(Some(adu));
    //     };
    //     Ok(None)
    // }
}

impl<R: Read> CompressedInput<R> {
    /// Create a new compressed input stream.
    pub fn new(delta_t_max: DeltaT, ref_interval: DeltaT, adu_interval: usize) -> Self
    where
        Self: Sized,
    {
        Self {
            meta: CodecMetadata {
                codec_version: 0,
                header_size: 0,
                time_mode: Default::default(),
                plane: Default::default(),
                tps: 0,
                ref_interval,
                delta_t_max,
                event_size: 0,
                source_camera: Default::default(),
                adu_interval,
            },
            adu: None,
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<R: Read + Seek> ReadCompression<R> for CompressedInput<R> {
    fn magic(&self) -> Magic {
        MAGIC_COMPRESSED
    }

    fn meta(&self) -> &CodecMetadata {
        &self.meta
    }

    fn meta_mut(&mut self) -> &mut CodecMetadata {
        &mut self.meta
    }

    fn read_bytes(
        &mut self,
        bytes: &mut [u8],
        reader: &mut BitReader<R, BigEndian>,
    ) -> std::io::Result<()> {
        reader.read_bytes(bytes)
    }

    // fn into_reader(self: Box<Self>, reader: &mut BitReader<R, BigEndian>) -> R {
    //     reader.into_reader()
    // }

    #[allow(unused_variables)]
    fn digest_event(&mut self, reader: &mut BitReader<R, BigEndian>) -> Result<Event, CodecError> {
        if self.adu.is_none() {
            self.adu = Some(EventAdu::new(
                self.meta.plane,
                0,
                self.meta.ref_interval,
                self.meta.adu_interval,
            ));
        }

        if let Some(adu) = &mut self.adu {
            if adu.decoder_is_empty() {
                let start = std::time::Instant::now();
                // Read the size of the Adu in bytes
                let mut buffer = [0u8; 4];
                reader.read_bytes(&mut buffer)?;
                let num_bytes = u32::from_be_bytes(buffer);

                // Read the compressed Adu from the stream
                let adu_bytes = reader.read_to_vec(num_bytes as usize)?;

                // Create a temporary u8 stream to read the arithmetic-coded data from
                let mut adu_stream = BitReader::endian(Cursor::new(adu_bytes), BigEndian);

                // Decompress the Adu
                adu.decompress(&mut adu_stream);

                let duration = start.elapsed();
                println!("Decompressed Adu in {:?} ns", duration.as_nanos());
            }
            // Then return the next event from the queue
            match adu.digest_event() {
                Ok(event) => Ok(event),
                Err(CodecError::NoMoreEvents) => {
                    // If there are no more events in the Adu, try decompressing the next Adu
                    self.digest_event(reader)
                }
                Err(e) => Err(e),
            }
        } else {
            unreachable!("Invalid state");
        }
    }

    #[allow(unused_variables)]
    fn set_input_stream_position(
        &mut self,
        reader: &mut BitReader<R, BigEndian>,
        pos: u64,
    ) -> Result<(), CodecError> {
        if pos.saturating_sub(self.meta.header_size as u64) % u64::from(self.meta.event_size) != 0 {
            eprintln!("Attempted to seek to bad position in stream: {pos}");
            return Err(CodecError::Seek);
        }

        if reader.seek_bits(SeekFrom::Start(pos * 8)).is_err() {
            return Err(CodecError::Seek);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::codec::compressed::stream::CompressedInput;
    use crate::codec::{CodecError, ReadCompression};
    use crate::PlaneSize;
    use bitstream_io::{BigEndian, BitReader};
    use std::cmp::min;
    use std::error::Error;
    use std::io;

    /// Test the creation a CompressedOutput and writing a bunch of events to it but NOT getting
    /// to the time where we have a full Adu. It will compress the last partial ADU.
    #[test]
    fn test_compress_empty() -> Result<(), Box<dyn Error>> {
        use crate::codec::compressed::stream::CompressedOutput;
        use crate::codec::WriteCompression;
        use crate::Coord;
        use crate::{Event, SourceCamera, TimeMode};
        use std::io::Cursor;

        let start_t = 0;
        let dt_ref = 255;
        let num_intervals = 10;

        let mut compressed_output = CompressedOutput::new(
            crate::codec::CodecMetadata {
                codec_version: 0,
                header_size: 0,
                time_mode: TimeMode::AbsoluteT,
                plane: PlaneSize {
                    width: 16,
                    height: 32,
                    channels: 1,
                },
                tps: 7650,
                ref_interval: dt_ref,
                delta_t_max: dt_ref * num_intervals,
                event_size: 0,
                source_camera: SourceCamera::FramedU8,
                adu_interval: num_intervals as usize,
            },
            Cursor::new(Vec::new()),
        );

        let mut counter = 0;
        for y in 0..30 {
            for x in 0..16 {
                compressed_output
                    .ingest_event(Event {
                        coord: Coord { x, y, c: None },
                        t: min(280 + counter, start_t + dt_ref * num_intervals as u32),
                        d: 7,
                    })
                    .unwrap();
                if 280 + counter > start_t + dt_ref * num_intervals as u32 {
                    break;
                } else {
                    counter += 1;
                }
            }
        }

        let output = compressed_output.into_writer().unwrap().into_inner();
        assert!(!output.is_empty());
        Ok(())
    }

    #[test]
    fn test_compress_decompress_barely_full() -> Result<(), Box<dyn Error>> {
        use crate::codec::compressed::stream::CompressedOutput;
        use crate::codec::WriteCompression;
        use crate::Coord;
        use crate::{Event, SourceCamera, TimeMode};
        use std::io::Cursor;

        let plane = PlaneSize::new(16, 30, 1)?;
        let start_t = 0;
        let dt_ref = 255;
        let num_intervals = 10;

        // A random candidate pixel to check that its events match
        let candidate_px_idx = (7, 12);
        let mut input_px_events = Vec::new();
        let mut output_px_events = Vec::new();

        let mut compressed_output = CompressedOutput::new(
            crate::codec::CodecMetadata {
                codec_version: 0,
                header_size: 0,
                time_mode: TimeMode::AbsoluteT,
                plane: PlaneSize {
                    width: 16,
                    height: 32,
                    channels: 1,
                },
                tps: 7650,
                ref_interval: dt_ref,
                delta_t_max: dt_ref * num_intervals,
                event_size: 0,
                source_camera: SourceCamera::FramedU8,
                adu_interval: num_intervals as usize,
            },
            Cursor::new(Vec::new()),
        );

        let mut counter = 0;
        for y in 0..30 {
            for x in 0..16 {
                let event = Event {
                    coord: Coord { x, y, c: None },
                    t: min(280 + counter, start_t + dt_ref * num_intervals as u32),
                    d: 7,
                };
                if y == candidate_px_idx.0 && x == candidate_px_idx.1 {
                    input_px_events.push(event);
                }
                compressed_output.ingest_event(event).unwrap();
                if 280 + counter > start_t + dt_ref * num_intervals as u32 {
                    break;
                } else {
                    counter += 1;
                }
            }
        }

        // Ingest one more event which is in the next Adu time span
        compressed_output
            .ingest_event(Event {
                coord: Coord {
                    x: 0,
                    y: 0,
                    c: None,
                },
                t: start_t + dt_ref * num_intervals as u32 + 1,
                d: 7,
            })
            .unwrap();
        counter += 1;

        // Sleep for 3 seconds to give the writer thread time to catch up
        std::thread::sleep(std::time::Duration::from_secs(3));

        let output = compressed_output.into_writer().unwrap().into_inner();
        assert!(!output.is_empty());
        dbg!(counter);
        dbg!(output.len());
        // Check that the size is less than the raw events
        assert!((output.len() as u32) < counter * 9);

        let mut compressed_input = CompressedInput::new(
            dt_ref * num_intervals as u32,
            dt_ref,
            num_intervals as usize,
        );
        compressed_input.meta.plane = plane;
        let mut stream = BitReader::endian(Cursor::new(output), BigEndian);
        for i in 0..counter - 1 {
            let event = compressed_input.digest_event(&mut stream);
            if event.is_err() {
                dbg!(i);
            }
            let event = event.unwrap();
            if event.coord.y == candidate_px_idx.0 && event.coord.x == candidate_px_idx.1 {
                output_px_events.push(event);
            }
        }

        assert_eq!(input_px_events, output_px_events);
        Ok(())
    }

    #[test]
    fn test_compress_decompress_several() -> Result<(), Box<dyn Error>> {
        use crate::codec::compressed::stream::CompressedOutput;
        use crate::codec::WriteCompression;
        use crate::Coord;
        use crate::{Event, SourceCamera, TimeMode};
        use std::io::Cursor;

        let plane = PlaneSize::new(16, 30, 1)?;
        let dt_ref = 255;
        let num_intervals = 5;

        // A random candidate pixel to check that its events match
        let candidate_px_idx = (7, 12);
        let mut input_px_events = Vec::new();
        let mut output_px_events = Vec::new();

        let mut compressed_output = CompressedOutput::new(
            crate::codec::CodecMetadata {
                codec_version: 0,
                header_size: 0,
                time_mode: TimeMode::AbsoluteT,
                plane: PlaneSize {
                    width: 16,
                    height: 32,
                    channels: 1,
                },
                tps: 7650,
                ref_interval: dt_ref,
                delta_t_max: dt_ref * num_intervals as u32,
                event_size: 0,
                source_camera: SourceCamera::FramedU8,
                adu_interval: num_intervals as usize,
            },
            Cursor::new(Vec::new()),
        );

        let mut counter = 0;
        for _ in 0..10 {
            for y in 0..30 {
                for x in 0..16 {
                    let event = Event {
                        coord: Coord { x, y, c: None },
                        t: 280 + counter,
                        d: 7,
                    };
                    if y == candidate_px_idx.0 && x == candidate_px_idx.1 {
                        input_px_events.push(event);
                    }
                    compressed_output.ingest_event(event)?;

                    counter += 1;
                }
            }
        }

        let output = compressed_output.into_writer().unwrap().into_inner();
        assert!(!output.is_empty());
        // Check that the size is less than the raw events
        assert!((output.len() as u32) < counter * 9);

        let mut compressed_input = CompressedInput::new(
            dt_ref * num_intervals as u32,
            dt_ref,
            num_intervals as usize,
        );
        compressed_input.meta.plane = plane;
        let mut stream = BitReader::endian(Cursor::new(output), BigEndian);
        for _ in 0..counter - 1 {
            match compressed_input.digest_event(&mut stream) {
                Ok(event) => {
                    if event.coord.y == candidate_px_idx.0 && event.coord.x == candidate_px_idx.1 {
                        output_px_events.push(event);
                    }
                }
                Err(CodecError::IoError(e)) if e.kind() == io::ErrorKind::UnexpectedEof => break,

                Err(e) => return Err(Box::new(e)),
            }
        }

        assert!(input_px_events.len() >= output_px_events.len());
        for i in 0..output_px_events.len() {
            // Have some slack in the comparison of the T component, since there could be some slight loss here
            let a = input_px_events[i].t - 5..input_px_events[i].t + 5;
            let comp_t = output_px_events[i].t;
            assert!(a.contains(&comp_t));
            assert_eq!(input_px_events[i].d, output_px_events[i].d);
        }
        Ok(())
    }

    #[test]
    fn test_compress_decompress_several_single() -> Result<(), Box<dyn Error>> {
        use crate::codec::compressed::stream::CompressedOutput;
        use crate::codec::WriteCompression;
        use crate::Coord;
        use crate::{Event, SourceCamera, TimeMode};
        use std::io::Cursor;

        let plane = PlaneSize::new(32, 16, 1)?;
        let dt_ref = 255;
        let num_intervals = 5;

        // A random candidate pixel to check that its events match
        let candidate_px_idx = (7, 12);
        let mut input_px_events = Vec::new();
        let mut output_px_events = Vec::new();

        let mut compressed_output = CompressedOutput::new(
            crate::codec::CodecMetadata {
                codec_version: 0,
                header_size: 0,
                time_mode: TimeMode::AbsoluteT,
                plane,
                tps: 7650,
                ref_interval: dt_ref,
                delta_t_max: dt_ref * num_intervals as u32,
                event_size: 0,
                source_camera: SourceCamera::FramedU8,
                adu_interval: num_intervals as usize,
            },
            Cursor::new(Vec::new()),
        );

        let mut counter = 0;
        for i in 0..60 {
            let event = Event {
                coord: Coord {
                    x: 12,
                    y: 7,
                    c: None,
                },
                t: 280 + i * 100 + counter,
                d: 7,
            };

            input_px_events.push(event);

            compressed_output.ingest_event(event)?;

            counter += 1;
        }

        // MUCH LATER, integrate an event that with a timestamp far in the past:
        let late_event = Event {
            coord: Coord {
                x: 19,
                y: 14,
                c: None,
            },
            t: 280,
            d: 7,
        };
        compressed_output.ingest_event(late_event)?;

        for i in 60..70 {
            let event = Event {
                coord: Coord {
                    x: 12,
                    y: 7,
                    c: None,
                },
                t: 280 + i * 100 + counter,
                d: 7,
            };

            input_px_events.push(event);

            compressed_output.ingest_event(event)?;

            counter += 1;
        }

        // Sleep for 3 seconds to give the writer thread time to catch up
        std::thread::sleep(std::time::Duration::from_secs(3));

        let output = compressed_output.into_writer().unwrap().into_inner();
        assert!(!output.is_empty());
        // Check that the size is less than the raw events

        let mut compressed_input = CompressedInput::new(
            dt_ref * num_intervals as u32,
            dt_ref,
            num_intervals as usize,
        );
        compressed_input.meta.plane = plane;
        let mut stream = BitReader::endian(Cursor::new(output), BigEndian);
        for _ in 0..counter + 1 {
            match compressed_input.digest_event(&mut stream) {
                Ok(event) => {
                    if event.coord.y == candidate_px_idx.0 && event.coord.x == candidate_px_idx.1 {
                        output_px_events.push(event);
                    }
                }
                Err(CodecError::IoError(e)) if e.kind() == io::ErrorKind::UnexpectedEof => break,

                Err(e) => return Err(Box::new(e)),
            }
        }

        assert!(input_px_events.len() >= output_px_events.len());
        for i in 0..output_px_events.len() {
            let span = input_px_events[i].t - 5..input_px_events[i].t + 5;
            let t = output_px_events[i].t;
            assert!(span.contains(&t));
        }
        Ok(())
    }

    #[test]
    fn test_compress_decompress_several_with_skip() -> Result<(), Box<dyn Error>> {
        use crate::codec::compressed::stream::CompressedOutput;
        use crate::codec::WriteCompression;
        use crate::Coord;
        use crate::{Event, SourceCamera, TimeMode};
        use std::io::Cursor;

        let plane = PlaneSize::new(30, 30, 1)?;
        let dt_ref = 255;
        let num_intervals = 10;

        // A random candidate pixel to check that its events match
        let candidate_px_idx = (7, 12);
        let mut input_px_events = Vec::new();
        let mut output_px_events = Vec::new();

        let mut compressed_output = CompressedOutput::new(
            crate::codec::CodecMetadata {
                codec_version: 0,
                header_size: 0,
                time_mode: TimeMode::AbsoluteT,
                plane,
                tps: 7650,
                ref_interval: dt_ref,
                delta_t_max: dt_ref * num_intervals as u32,
                event_size: 0,
                source_camera: SourceCamera::FramedU8,
                adu_interval: num_intervals as usize,
            },
            Cursor::new(Vec::new()),
        );

        let mut counter = 0;
        for i in 0..10 {
            for y in 0..30 {
                for x in 0..30 {
                    // Make the top left cube a skip cube half the time, and skip pixel (14, 14)
                    if !(y == 14 && x == 14 || i % 3 == 0 && y >= 16 && x < 16) {
                        let event = Event {
                            coord: Coord { x, y, c: None },
                            t: 280 + counter,
                            d: 7,
                        };
                        if y == candidate_px_idx.0 && x == candidate_px_idx.1 {
                            input_px_events.push(event);
                        }
                        compressed_output.ingest_event(event)?;

                        counter += 1;
                    }
                }
            }
        }

        // MUCH LATER, integrate an event that with a timestamp far in the past:
        let late_event = Event {
            coord: Coord {
                x: 14,
                y: 14,
                c: None,
            },
            t: 280,
            d: 7,
        };
        compressed_output.ingest_event(late_event)?;

        for i in 0..10 {
            for y in 0..30 {
                for x in 0..30 {
                    // Make the top left cube a skip cube half the time, and skip pixel (14, 14)
                    if !(y == 14 && x == 14 || i % 3 == 0 && y >= 16 && x < 16) {
                        let event = Event {
                            coord: Coord { x, y, c: None },
                            t: 280 + counter,
                            d: 7,
                        };
                        if y == candidate_px_idx.0 && x == candidate_px_idx.1 {
                            input_px_events.push(event);
                        }
                        compressed_output.ingest_event(event)?;

                        counter += 1;
                    }
                }
            }
        }

        // Sleep for 3 seconds to give the writer thread time to catch up
        std::thread::sleep(std::time::Duration::from_secs(10));

        let output = compressed_output.into_writer().unwrap().into_inner();
        assert!(!output.is_empty());
        // Check that the size is less than the raw events
        assert!((output.len() as u32) < counter * 9);

        let mut compressed_input = CompressedInput::new(
            dt_ref * num_intervals as u32,
            dt_ref,
            num_intervals as usize,
        );
        compressed_input.meta.plane = plane;
        let mut stream = BitReader::endian(Cursor::new(output), BigEndian);
        loop {
            match compressed_input.digest_event(&mut stream) {
                Ok(event) => {
                    if event.coord.y == candidate_px_idx.0 && event.coord.x == candidate_px_idx.1 {
                        output_px_events.push(event);
                    }
                }
                Err(CodecError::IoError(e)) if e.kind() == io::ErrorKind::UnexpectedEof => break,

                Err(e) => return Err(Box::new(e)),
            }
        }

        assert!(input_px_events.len() >= output_px_events.len());
        for i in 0..output_px_events.len() {
            // Have some slack in the comparison of the T component, since there could be some slight loss here
            let a = input_px_events[i].t - 5..input_px_events[i].t + 5;
            let comp_t = output_px_events[i].t;
            assert!(a.contains(&comp_t));
            assert_eq!(input_px_events[i].d, output_px_events[i].d);
        }
        Ok(())
    }
}
