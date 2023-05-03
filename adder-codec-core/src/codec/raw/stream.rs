use crate::codec::compressed::adu::frame::Adu;
use crate::codec::header::{Magic, MAGIC_RAW};
use crate::codec::{CodecError, CodecMetadata, ReadCompression, WriteCompression};
use crate::{Coord, DeltaT, Event, EventSingle, EOF_PX_ADDRESS};
use bincode::config::{FixintEncoding, WithOtherEndian, WithOtherIntEncoding};
use bincode::{DefaultOptions, Options};
use bitstream_io::{BigEndian, BitRead, BitReader};
use hashbrown::hash_map::DefaultHashBuilder;
use priority_queue::PriorityQueue;
use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::io::{Read, Seek, SeekFrom, Write};

/// Write uncompressed (raw) ADΔER data to a stream.
pub struct RawOutput<W> {
    pub(crate) meta: CodecMetadata,
    pub(crate) bincode: WithOtherEndian<
        WithOtherIntEncoding<DefaultOptions, FixintEncoding>,
        bincode::config::BigEndian,
    >,
    queue: BinaryHeap<Event>,
    pub(crate) stream: Option<W>,
}

/// Read uncompressed (raw) ADΔER data from a stream.
pub struct RawInput<R: Read + Seek> {
    pub(crate) meta: CodecMetadata,
    pub(crate) bincode: WithOtherEndian<
        WithOtherIntEncoding<DefaultOptions, FixintEncoding>,
        bincode::config::BigEndian,
    >,
    _phantom: std::marker::PhantomData<R>,
}

impl<W: Write> RawOutput<W> {
    /// Create a new raw output stream.
    pub fn new(mut meta: CodecMetadata, writer: W) -> Self {
        let bincode = DefaultOptions::new()
            .with_fixint_encoding()
            .with_big_endian();
        meta.event_size = match meta.plane.c() {
            1 => bincode.serialized_size(&EventSingle::default()).unwrap() as u8,
            _ => bincode.serialized_size(&Event::default()).unwrap() as u8,
        };
        Self {
            meta,
            bincode,
            queue: BinaryHeap::new(),
            stream: Some(writer),
        }
    }

    fn stream(&mut self) -> &mut W {
        self.stream.as_mut().unwrap()
    }
}

impl<W: Write> WriteCompression<W> for RawOutput<W> {
    fn magic(&self) -> Magic {
        MAGIC_RAW
    }

    fn meta(&self) -> &CodecMetadata {
        &self.meta
    }

    fn meta_mut(&mut self) -> &mut CodecMetadata {
        &mut self.meta
    }

    fn write_bytes(&mut self, bytes: &[u8]) -> Result<(), std::io::Error> {
        // Silently ignore the returned usize because we don't care about the number of bytes
        self.stream().write(bytes).map(|_| ())
    }

    // Will always be byte-aligned. Do nothing.
    fn byte_align(&mut self) -> std::io::Result<()> {
        Ok(())
    }

    // If `self.writer` is a `BufWriter`, you'll need to flush it yourself after this.
    fn into_writer(&mut self) -> Option<W> {
        dbg!("IN INTO_WRITER!");
        while let Some(first_item) = self.queue.pop() {
            let output_event: EventSingle;
            if self.meta.plane.channels == 1 {
                // let event_to_write = self.queue.pop()
                output_event = (&first_item).into();
                self.bincode
                    .serialize_into(self.stream(), &output_event)
                    .unwrap();
                // bincode::serialize_into(&mut *stream, &output_event, my_options).unwrap();
            } else {
                self.bincode
                    .serialize_into(self.stream(), &first_item)
                    .unwrap();
            }
        }
        let eof = Event {
            coord: Coord {
                x: EOF_PX_ADDRESS,
                y: EOF_PX_ADDRESS,
                c: Some(0),
            },
            d: 0,
            delta_t: 0,
        };
        self.bincode.serialize_into(self.stream(), &eof).unwrap();
        self.flush_writer().unwrap();
        std::mem::replace(&mut self.stream, None)
    }

    fn flush_writer(&mut self) -> std::io::Result<()> {
        self.stream().flush()
    }

    /// Ingest an event into the codec_old.
    ///
    /// This will always write the event immediately to the underlying writer.
    fn ingest_event(&mut self, event: Event) -> Result<(), CodecError> {
        // NOTE: for speed, the following checks only run in debug builds. It's entirely
        // possibly to encode nonsensical events if you want to.
        debug_assert!(event.coord.x < self.meta.plane.width || event.coord.x == EOF_PX_ADDRESS);
        debug_assert!(event.coord.y < self.meta.plane.height || event.coord.y == EOF_PX_ADDRESS);

        // TODO: Switch functionality based on what the deltat mode is!

        // First, push the event to the queue
        let dt = event.delta_t;
        self.queue.push(event);

        if let Some(first_item_addr) = self.queue.peek() {
            if first_item_addr.delta_t < dt.saturating_sub(self.meta.delta_t_max) {
                if let Some(first_item) = self.queue.pop() {
                    let output_event: EventSingle;
                    if self.meta.plane.channels == 1 {
                        // let event_to_write = self.queue.pop()
                        output_event = (&first_item).into();
                        self.bincode.serialize_into(self.stream(), &output_event)?;
                        // bincode::serialize_into(&mut *stream, &output_event, my_options).unwrap();
                    } else {
                        self.bincode.serialize_into(self.stream(), &first_item)?;
                    }
                }
            }
        }

        Ok(())
    }

    fn ingest_event_debug(&mut self, event: Event) -> Result<Option<Adu>, CodecError> {
        todo!()
    }
}

impl<R: Read + Seek> RawInput<R> {
    /// Create a new raw input stream.
    pub fn new() -> Self
    where
        Self: Sized,
    {
        Self {
            meta: CodecMetadata::default(),
            bincode: DefaultOptions::new()
                .with_fixint_encoding()
                .with_big_endian(),
            // stream: reader,
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<R: Read + Seek> ReadCompression<R> for RawInput<R> {
    fn magic(&self) -> Magic {
        MAGIC_RAW
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

    #[inline]
    fn digest_event(&mut self, reader: &mut BitReader<R, BigEndian>) -> Result<Event, CodecError> {
        // TODO: Why is the encoded event size wrong?
        let mut buffer: Vec<u8> = vec![0; self.meta.event_size as usize];
        reader.read_bytes(&mut buffer)?;
        let event: Event = if self.meta.plane.channels == 1 {
            match self.bincode.deserialize_from::<_, EventSingle>(&*buffer) {
                Ok(ev) => ev.into(),
                Err(_e) => return Err(CodecError::Deserialize),
            }
        } else {
            match self.bincode.deserialize_from::<_, Event>(&*buffer) {
                Ok(ev) => ev,
                Err(e) => {
                    dbg!(self.meta.event_size);
                    eprintln!("Error deserializing event: {e}");
                    return Err(CodecError::Deserialize);
                }
            }
        };

        if event.coord.is_eof() {
            return Err(CodecError::Eof);
        }
        Ok(event)
    }

    fn digest_event_debug(
        &mut self,
        reader: &mut BitReader<R, BigEndian>,
    ) -> Result<(Option<Adu>, Event), CodecError> {
        todo!()
    }

    fn set_input_stream_position(
        &mut self,
        reader: &mut BitReader<R, BigEndian>,
        pos: u64,
    ) -> Result<(), CodecError> {
        if (pos - self.meta.header_size as u64) % u64::from(self.meta.event_size) != 0 {
            eprintln!("Attempted to seek to bad position in stream: {pos}");
            return Err(CodecError::Seek);
        }

        if reader.seek_bits(SeekFrom::Start(pos * 8)).is_err() {
            return Err(CodecError::Seek);
        }

        Ok(())
    }
}
