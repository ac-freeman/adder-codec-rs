use crate::codec::header::{Magic, MAGIC_RAW};
use crate::codec::{CodecError, CodecMetadata, ReadCompression, WriteCompression};
use crate::{Event, EventSingle, EOF_PX_ADDRESS, Coord};
use bincode::config::{FixintEncoding, WithOtherEndian, WithOtherIntEncoding};
use bincode::{DefaultOptions, Options};
use bitstream_io::{BigEndian, BitRead, BitReader};
use std::io::{Read, Seek, SeekFrom, Write};

pub struct RawOutput<W> {
    pub(crate) meta: CodecMetadata,
    pub(crate) bincode: WithOtherEndian<
        WithOtherIntEncoding<DefaultOptions, FixintEncoding>,
        bincode::config::BigEndian,
    >,
    pub(crate) stream: W,
}

pub struct RawInput {
    pub(crate) meta: CodecMetadata,
    pub(crate) bincode: WithOtherEndian<
        WithOtherIntEncoding<DefaultOptions, FixintEncoding>,
        bincode::config::BigEndian,
    >,
}

impl<W: Write> WriteCompression<W> for RawOutput<W> {
    fn new(meta: CodecMetadata, writer: W) -> Self {
        Self {
            meta,
            bincode: DefaultOptions::new()
                .with_fixint_encoding()
                .with_big_endian(),
            stream: writer,
        }
    }

    fn magic(&self) -> Magic {
        MAGIC_RAW
    }

    fn meta(&self) -> &CodecMetadata {
        &self.meta
    }

    fn meta_mut(&mut self) -> &mut CodecMetadata {
        &mut self.meta
    }

    fn write_bytes(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        // Silently ignore the returned usize because we don't care about the number of bytes
        self.stream.write(bytes).map(|_| ())
    }

    // Will always be byte-aligned. Do nothing.
    fn byte_align(&mut self) -> std::io::Result<()> {
        Ok(())
    }

    /// If `self.writer` is a `BufWriter`, you'll need to flush it yourself after this.
    fn into_writer(self: Box<Self>) -> W {
        self.stream
    }

    fn flush_writer(&mut self) -> std::io::Result<()> {
        self.stream.flush()
    }

    fn compress(&self, _data: &[u8]) -> Vec<u8> {
        todo!()
    }

    /// Ingest an event into the codec.
    ///
    /// This will always write the event immediately to the underlying writer.
    fn ingest_event(&mut self, event: &Event) -> Result<(), CodecError> {
        // NOTE: for speed, the following checks only run in debug builds. It's entirely
        // possibly to encode nonsensical events if you want to.
        debug_assert!(event.coord.x < self.meta.plane.width || event.coord.x == EOF_PX_ADDRESS);
        debug_assert!(event.coord.y < self.meta.plane.height || event.coord.y == EOF_PX_ADDRESS);
        let output_event: EventSingle;
        if self.meta.plane.channels == 1 {
            output_event = event.into();
            self.bincode
                .serialize_into(&mut self.stream, &output_event)?;
            // bincode::serialize_into(&mut *stream, &output_event, my_options).unwrap();
        } else {
            self.bincode.serialize_into(&mut self.stream, event)?;
        }
        Ok(())
    }
}

impl<R: Read + Seek> ReadCompression<R> for RawInput {
    fn new() -> Self
    where
        Self: Sized,
    {
        Self {
            meta: CodecMetadata::default(),
            bincode: DefaultOptions::new()
                .with_fixint_encoding()
                .with_big_endian(),
            // stream: reader,
        }
    }

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

    fn digest_event(&mut self, reader: &mut BitReader<R, BigEndian>) -> Result<Event, CodecError> {
        // TODO: Why is the encoded event size wrong?
        let mut buffer: Vec<u8> = vec![0; self.meta.event_size as usize];
        reader.read_bytes(&mut buffer);
        let event: Event = if self.meta.plane.channels == 1 {
            match self.bincode.deserialize_from::<_, EventSingle>(&*buffer) {
                Ok(ev) => ev.into(),
                Err(_e) => return Err(CodecError::Deserialize),
            }
        } else {
            match self.bincode.deserialize_from(&*buffer) {
                Ok(ev) => ev,
                Err(e) => {
                    eprintln!("Error deserializing event: {}", e);
                    return Err(CodecError::Deserialize)
                }
            }
        };

        if event.coord.y == EOF_PX_ADDRESS && event.coord.x == EOF_PX_ADDRESS {
            return Err(CodecError::Eof);
        }
        Ok(event)
    }

    fn set_input_stream_position(
        &mut self,
        reader: &mut BitReader<R, BigEndian>,
        pos: u64,
    ) -> Result<(), CodecError> {
        if (pos - self.meta.header_size as u64) % u64::from(self.meta.event_size) != 0 {
            eprintln!("Attempted to seek to bad position in stream: {}", pos);
            return Err(CodecError::Seek);
        }

        if reader.seek_bits(SeekFrom::Start(pos * 8)).is_err() {
            return Err(CodecError::Seek);
        }

        Ok(())
    }
}
