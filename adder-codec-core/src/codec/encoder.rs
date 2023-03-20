use crate::codec::{CodecError, CodecMetadata, WriteCompression, WriteCompressionEnum};
use crate::SourceType::*;
use crate::{Event, EventSingle, SourceCamera, SourceType, EOF_EVENT};

use std::io;
use std::io::{Sink, Write};

use crate::codec::compressed::stream::CompressedOutput;
use crate::codec::empty::stream::EmptyOutput;
use crate::codec::header::{
    EventStreamHeader, EventStreamHeaderExtensionV0, EventStreamHeaderExtensionV1,
    EventStreamHeaderExtensionV2,
};
use crate::codec::raw::stream::RawOutput;
use crate::SourceType::U8;
use bincode::config::{FixintEncoding, WithOtherEndian, WithOtherIntEncoding};
use bincode::{DefaultOptions, Options};

/// Struct for encoding [`Event`]s to a stream
pub struct Encoder<W: Write> {
    output: WriteCompressionEnum<W>,
    bincode: WithOtherEndian<
        WithOtherIntEncoding<DefaultOptions, FixintEncoding>,
        bincode::config::BigEndian,
    >,
}

#[allow(dead_code)]
impl<W: Write + 'static> Encoder<W> {
    /// Create a new [`Encoder`] with an empty compression scheme
    pub fn new_empty(compression: EmptyOutput<Sink>) -> Self
    where
        Self: Sized,
    {
        let mut encoder = Self {
            output: WriteCompressionEnum::EmptyOutput(compression),
            bincode: DefaultOptions::new()
                .with_fixint_encoding()
                .with_big_endian(),
        };
        encoder.encode_header().unwrap();
        encoder
    }

    /// Create a new [`Encoder`] with the given compression scheme
    pub fn new_compressed(compression: CompressedOutput<W>) -> Self
    where
        Self: Sized,
    {
        let mut encoder = Self {
            output: WriteCompressionEnum::CompressedOutput(compression),
            bincode: DefaultOptions::new()
                .with_fixint_encoding()
                .with_big_endian(),
        };
        encoder.encode_header().unwrap();
        encoder
    }

    /// Create a new [`Encoder`] with the given raw compression scheme
    pub fn new_raw(compression: RawOutput<W>) -> Self
    where
        Self: Sized,
    {
        let mut encoder = Self {
            output: WriteCompressionEnum::RawOutput(compression),
            bincode: DefaultOptions::new()
                .with_fixint_encoding()
                .with_big_endian(),
        };
        encoder.encode_header().unwrap();
        encoder
    }

    /// Returns a reference to the metadata of the underlying compression scheme
    #[inline]
    pub fn meta(&self) -> &CodecMetadata {
        self.output.meta()
    }

    #[allow(clippy::match_same_arms)]
    fn get_source_type(&self) -> SourceType {
        match self.output.meta().source_camera {
            SourceCamera::FramedU8 => U8,
            SourceCamera::FramedU16 => U16,
            SourceCamera::FramedU32 => U32,
            SourceCamera::FramedU64 => U64,
            SourceCamera::FramedF32 => F32,
            SourceCamera::FramedF64 => F64,
            SourceCamera::Dvs => F64,
            SourceCamera::DavisU8 => U8,
            SourceCamera::Atis => U8,
            SourceCamera::Asint => F64,
        }
    }

    /// Signify the end of the file in a unified way
    fn write_eof(&mut self) -> Result<(), CodecError> {
        self.output.byte_align()?;
        let output_event: EventSingle;
        let mut buffer = Vec::new();
        if self.output.meta().plane.channels == 1 {
            output_event = (&EOF_EVENT).into();
            self.bincode.serialize_into(&mut buffer, &output_event)?;
        } else {
            self.bincode.serialize_into(&mut buffer, &EOF_EVENT)?;
        }
        Ok(self.output.write_bytes(&buffer)?)
    }

    /// Flush the `BitWriter`. Does not flush the internal `BufWriter`.
    pub fn flush_writer(&mut self) -> io::Result<()> {
        self.output.flush_writer()
    }

    /// Close the encoder's writer and return it, consuming the encoder in the process.
    pub fn close_writer(mut self) -> Result<Option<W>, CodecError> {
        self.output.byte_align()?;
        self.write_eof()?;
        self.flush_writer()?;
        Ok(self.output.into_writer())
        // let compressed_output = self.compressed_output.take();
        // let raw_output = self.raw_output.take();
        //
        // if compressed_output.is_some() {
        //     return Ok(compressed_output.unwrap().into_writer());
        // } else if raw_output.is_some() {
        //     return Ok(raw_output.unwrap().into_writer());
        // } else {
        //     unreachable!()
        // }
    }

    /// Encode the header and its extensions.
    fn encode_header(&mut self) -> Result<(), CodecError> {
        let mut buffer: Vec<u8> = Vec::new();
        let meta = self.output.meta();
        let header = EventStreamHeader::new(
            self.output.magic(),
            meta.plane,
            meta.tps,
            meta.ref_interval,
            meta.delta_t_max,
            meta.codec_version,
        );
        self.bincode.serialize_into(&mut buffer, &header)?;

        // Encode the header extensions (for newer versions of the codec_old)
        buffer = self.encode_header_extension(buffer)?;

        self.output.write_bytes(&buffer)?;
        self.output.meta_mut().header_size = buffer.len();
        Ok(())
    }

    fn encode_header_extension(&self, mut buffer: Vec<u8>) -> Result<Vec<u8>, CodecError> {
        let meta = self.output.meta();
        self.bincode
            .serialize_into(&mut buffer, &EventStreamHeaderExtensionV0 {})?;
        if meta.codec_version == 0 {
            return Ok(buffer);
        }

        self.bincode.serialize_into(
            &mut buffer,
            &EventStreamHeaderExtensionV1 {
                source: meta.source_camera,
            },
        )?;
        if meta.codec_version == 1 {
            return Ok(buffer);
        }

        self.bincode.serialize_into(
            &mut buffer,
            &EventStreamHeaderExtensionV2 {
                time_mode: meta.time_mode,
            },
        )?;
        if meta.codec_version == 2 {
            return Ok(buffer);
        }
        Err(CodecError::BadFile)
    }

    /// Ingest an event
    pub fn ingest_event(&mut self, event: Event) -> Result<(), CodecError> {
        self.output.ingest_event(event)
    }

    /// Ingest an array of events
    ///
    /// TODO: Make this move events, not by reference
    pub fn ingest_events(&mut self, events: &[Event]) -> Result<(), CodecError> {
        for event in events {
            self.ingest_event(*event)?;
        }
        Ok(())
    }

    /// Ingest a vector of an array of events
    pub fn ingest_events_events(&mut self, events: &[Vec<Event>]) -> Result<(), CodecError> {
        for v in events {
            self.ingest_events(v)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::compressed::stream::CompressedOutput;
    use crate::codec::raw::stream::RawOutput;
    use crate::codec::{CodecMetadata, LATEST_CODEC_VERSION};

    use crate::{Coord, PlaneSize};
    use bitstream_io::{BigEndian, BitWriter};
    use std::io::BufWriter;

    #[test]
    fn raw() {
        let output = Vec::new();
        let bufwriter = BufWriter::new(output);
        let compression = RawOutput {
            meta: CodecMetadata {
                codec_version: 0,
                header_size: 0,
                time_mode: Default::default(),
                plane: Default::default(),
                tps: 0,
                ref_interval: 0,
                delta_t_max: 0,
                event_size: 0,
                source_camera: Default::default(),
            },
            bincode: DefaultOptions::new()
                .with_fixint_encoding()
                .with_big_endian(),
            stream: Some(bufwriter),
        };
        let encoder = Encoder {
            output: WriteCompressionEnum::RawOutput(compression),
            bincode: DefaultOptions::new()
                .with_fixint_encoding()
                .with_big_endian(),
        };
        let mut writer = encoder.close_writer().unwrap().unwrap();
        writer.flush().unwrap();
        let _output = writer.into_inner().unwrap();
    }

    #[test]
    fn raw2() {
        let output = Vec::new();
        let bufwriter = BufWriter::new(output);
        let compression = RawOutput::new(
            CodecMetadata {
                codec_version: 1,
                header_size: 0,
                time_mode: Default::default(),
                plane: Default::default(),
                tps: 0,
                ref_interval: 0,
                delta_t_max: 0,
                event_size: 0,
                source_camera: Default::default(),
            },
            bufwriter,
        );
        let encoder = Encoder {
            output: WriteCompressionEnum::RawOutput(compression),
            bincode: DefaultOptions::new()
                .with_fixint_encoding()
                .with_big_endian(),
        };
        let mut writer = encoder.close_writer().unwrap().unwrap();
        writer.flush().unwrap();
        let _output = writer.into_inner().unwrap();
    }

    #[test]
    fn raw3() {
        let output = Vec::new();
        let bufwriter = BufWriter::new(output);
        let compression = RawOutput::new(
            CodecMetadata {
                codec_version: LATEST_CODEC_VERSION,
                header_size: 0,
                time_mode: Default::default(),
                plane: PlaneSize {
                    width: 1,
                    height: 1,
                    channels: 3,
                },
                tps: 0,
                ref_interval: 255,
                delta_t_max: 255,
                event_size: 0,
                source_camera: Default::default(),
            },
            bufwriter,
        );
        let mut encoder: Encoder<BufWriter<Vec<u8>>> = Encoder::new_raw(compression);

        let event = Event {
            coord: Coord {
                x: 0,
                y: 0,
                c: Some(0),
            },
            d: 0,
            delta_t: 0,
        };

        encoder.ingest_event(event).unwrap();
        let mut writer = encoder.close_writer().unwrap().unwrap();
        writer.flush().unwrap();
        let output = writer.into_inner().unwrap();
        assert_eq!(output.len(), 33 + 22); // 33 bytes for the header, 22 bytes for the 2 events
    }

    #[test]
    fn compressed() {
        let output = Vec::new();
        let bufwriter = BufWriter::new(output);
        let compression = CompressedOutput {
            meta: CodecMetadata {
                codec_version: 0,
                header_size: 0,
                time_mode: Default::default(),
                plane: Default::default(),
                tps: 0,
                ref_interval: 0,
                delta_t_max: 0,
                event_size: 0,
                source_camera: Default::default(),
            },
            frame: Default::default(),
            arithmetic_coder: None,
            contexts: None,
            stream: Some(BitWriter::endian(bufwriter, BigEndian)),
        };
        let _encoder = Encoder {
            output: WriteCompressionEnum::CompressedOutput(compression),
            bincode: DefaultOptions::new()
                .with_fixint_encoding()
                .with_big_endian(),
        };
    }

    #[test]
    fn compressed2() {
        let output = Vec::new();
        let bufwriter = BufWriter::new(output);
        let compression = CompressedOutput::new(
            CodecMetadata {
                codec_version: 0,
                header_size: 0,
                time_mode: Default::default(),
                plane: Default::default(),
                tps: 0,
                ref_interval: 0,
                delta_t_max: 0,
                event_size: 0,
                source_camera: Default::default(),
            },
            bufwriter,
        );
        let _encoder = Encoder {
            output: WriteCompressionEnum::CompressedOutput(compression),
            bincode: DefaultOptions::new()
                .with_fixint_encoding()
                .with_big_endian(),
        };
    }

    #[test]
    fn compressed3() {
        let output = Vec::new();
        let bufwriter = BufWriter::new(output);
        let compression = CompressedOutput::new(
            CodecMetadata {
                codec_version: LATEST_CODEC_VERSION,
                header_size: 0,
                time_mode: Default::default(),
                plane: Default::default(),
                tps: 0,
                ref_interval: 255,
                delta_t_max: 255,
                event_size: 0,
                source_camera: Default::default(),
            },
            bufwriter,
        );
        let _encoder = Encoder::new_compressed(compression);
    }
}
