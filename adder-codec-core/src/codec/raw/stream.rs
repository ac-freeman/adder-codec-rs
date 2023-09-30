#[cfg(feature = "compression")]
use crate::codec::compressed::adu::frame::Adu;
use crate::codec::header::{Magic, MAGIC_RAW};
use crate::codec::{CodecError, CodecMetadata, ReadCompression, WriteCompression};
use crate::{Coord, Event, EventSingle, EOF_PX_ADDRESS};
use bincode::config::{FixintEncoding, WithOtherEndian, WithOtherIntEncoding};
use bincode::{DefaultOptions, Options};
use bitstream_io::{BigEndian, BitRead, BitReader};
use std::collections::BinaryHeap;
use std::io::{Read, Seek, SeekFrom, Write};
use std::time::Instant;

/// Write uncompressed (raw) ADΔER data to a stream.
pub struct RawOutput<W> {
    pub(crate) meta: CodecMetadata,
    pub(crate) bincode: WithOtherEndian<
        WithOtherIntEncoding<DefaultOptions, FixintEncoding>,
        bincode::config::BigEndian,
    >,
    pub(crate) stream: Option<W>,
}

/// Write uncompressed (raw) ADΔER data to a stream.
pub struct RawOutputInterleaved<W> {
    pub(crate) meta: CodecMetadata,
    pub(crate) bincode: WithOtherEndian<
        WithOtherIntEncoding<DefaultOptions, FixintEncoding>,
        bincode::config::BigEndian,
    >,
    queue: BinaryHeap<Event>,
    pub(crate) stream: Option<W>,
}


/// Write uncompressed (raw) ADΔER data to a stream but make it bandwidth limited
pub struct RawOutputBandwidthLimited<W> {
    pub(crate) meta: CodecMetadata,
    pub(crate) bincode: WithOtherEndian<
        WithOtherIntEncoding<DefaultOptions, FixintEncoding>,
        bincode::config::BigEndian,
    >,
    target_bitrate: f64,
    /// This is basically the decay rate.
    /// Should be between 0 and 1.
    alpha: f64,
    current_bitrate: f64,
    last_event: Instant,
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
        self.stream.take()
    }

    fn flush_writer(&mut self) -> std::io::Result<()> {
        self.stream().flush()
    }

    /// Ingest an event into the codec.
    ///
    /// This will always write the event immediately to the underlying writer.
    fn ingest_event(&mut self, event: Event) -> Result<(), CodecError> {
        // NOTE: for speed, the following checks only run in debug builds. It's entirely
        // possibly to encode nonsensical events if you want to.
        debug_assert!(event.coord.x < self.meta.plane.width || event.coord.x == EOF_PX_ADDRESS);
        debug_assert!(event.coord.y < self.meta.plane.height || event.coord.y == EOF_PX_ADDRESS);

        // TODO: Switch functionality based on what the deltat mode is!

        let output_event: EventSingle;
        if self.meta.plane.channels == 1 {
            // let event_to_write = self.queue.pop()
            output_event = (&event).into();
            self.bincode.serialize_into(self.stream(), &output_event)?;
            // bincode::serialize_into(&mut *stream, &output_event, my_options).unwrap();
        } else {
            self.bincode.serialize_into(self.stream(), &event)?;
        }

        Ok(())
    }

    #[cfg(feature = "compression")]
    fn ingest_event_debug(&mut self, event: Event) -> Result<Option<Adu>, CodecError> {
        todo!()
    }
}

// TODO: wip
impl<W: Write> RawOutputBandwidthLimited<W> {
    /// Create a new raw bandwidth limited output stream.
    pub fn new(mut meta: CodecMetadata, writer: W, target_bitrate: f64, alpha: f64) -> Self {
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
            target_bitrate,
            alpha,
            current_bitrate: 0.0,
            last_event: Instant::now(),
            stream: Some(writer),
        }
    }

    fn stream(&mut self) -> &mut W {
        self.stream.as_mut().unwrap()
    }
}

impl<W: Write> WriteCompression<W> for RawOutputBandwidthLimited<W> {
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
        self.flush_writer().unwrap();=
        self.stream.take()
    }

    fn flush_writer(&mut self) -> std::io::Result<()> {
        self.stream().flush()
    }

    /// Ingest an event into the codec_old.
    fn ingest_event(&mut self, event: Event) -> Result<(), CodecError> {
        // TODO: Eric
        // NOTE: for speed, the following checks only run in debug builds. It's entirely
        // possibly to encode nonsensical events if you want to.
        debug_assert!(event.coord.x < self.meta.plane.width || event.coord.x == EOF_PX_ADDRESS);
        debug_assert!(event.coord.y < self.meta.plane.height || event.coord.y == EOF_PX_ADDRESS);

        // TODO: Switch functionality based on what the deltat mode is!
        // ^ I don't know what that means -Eric
        let now = Instant::now();
        // it could be faster to use as_nanos here
        let t_diff = now.duration_since(self.last_event).as_secs_f64();

        let new_bitrate = self.alpha * self.current_bitrate + (1.0 - self.alpha) / t_diff;

        if new_bitrate > self.target_bitrate {
            //dbg!("Skipping event!");
            self.current_bitrate = self.alpha * self.current_bitrate;
            return Ok(()); // skip this event
        }

        //dbg!("Not skipping event!");
        //dbg!("{}", new_bitrate);

        self.last_event = now; // update time
        self.current_bitrate = new_bitrate;

        let output_event: EventSingle;
        if self.meta.plane.channels == 1 {
            // let event_to_write = self.queue.pop()
            output_event = (&event).into();
            self.bincode.serialize_into(self.stream(), &output_event)?;
            // bincode::serialize_into(&mut *stream, &output_event, my_options).unwrap();
        } else {
            self.bincode.serialize_into(self.stream(), &event)?;
        }

        Ok(())
    }

    #[cfg(feature = "compression")]
    fn ingest_event_debug(&mut self, event: Event) -> Result<Option<Adu>, CodecError> {
        todo!()
    }
}

impl<W: Write> RawOutputInterleaved<W> {
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

impl<W: Write> WriteCompression<W> for RawOutputInterleaved<W> {
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
        self.stream.take()
    }

    fn flush_writer(&mut self) -> std::io::Result<()> {
        self.stream().flush()
    }

    /// Ingest an event into the codec.
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

    #[cfg(feature = "compression")]
    fn ingest_event_debug(&mut self, event: Event) -> Result<Option<Adu>, CodecError> {
        todo!()
    }
}

impl<R: Read + Seek> Default for RawInput<R> {
    fn default() -> Self {
        Self::new()
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


    #[cfg(feature = "compression")]
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
