use crate::codec::{
    CodecError, CodecMetadata, EncoderOptions, EventDrop, EventOrder, WriteCompression,
    WriteCompressionEnum,
};
use crate::SourceType::*;
use crate::{Event, EventSingle, SourceCamera, SourceType, EOF_EVENT};
use std::collections::BinaryHeap;

use std::io;
use std::io::{Sink, Write};
use std::time::Instant;

// #[cfg(feature = "compression")]
// use crate::codec::compressed::adu::frame::Adu;
#[cfg(feature = "compression")]
use crate::codec::compressed::stream::CompressedOutput;

use crate::codec::empty::stream::EmptyOutput;
use crate::codec::header::{
    EventStreamHeader, EventStreamHeaderExtensionV0, EventStreamHeaderExtensionV1,
    EventStreamHeaderExtensionV2, EventStreamHeaderExtensionV3,
};

use crate::codec::raw::stream::RawOutput;
use bincode::config::{FixintEncoding, WithOtherEndian, WithOtherIntEncoding};
use bincode::{DefaultOptions, Options};

/// Struct for encoding [`Event`]s to a stream
pub struct Encoder<W: Write + std::marker::Send + std::marker::Sync + 'static> {
    output: WriteCompressionEnum<W>,
    bincode: WithOtherEndian<
        WithOtherIntEncoding<DefaultOptions, FixintEncoding>,
        bincode::config::BigEndian,
    >,
    pub options: EncoderOptions,
    state: EncoderState,
}

struct EncoderState {
    current_event_rate: f64,
    last_event_ts: Instant,
    queue: BinaryHeap<Event>,
}

impl Default for EncoderState {
    fn default() -> Self {
        Self {
            current_event_rate: 0.0,
            last_event_ts: Instant::now(),
            queue: BinaryHeap::new(),
        }
    }
}

#[allow(dead_code)]
impl<W: Write + 'static + std::marker::Send + std::marker::Sync> Encoder<W> {
    /// Create a new [`Encoder`] with an empty compression scheme
    pub fn new_empty(compression: EmptyOutput<Sink>, options: EncoderOptions) -> Self
    where
        Self: Sized,
    {
        let mut encoder = Self {
            output: WriteCompressionEnum::EmptyOutput(compression),
            bincode: DefaultOptions::new()
                .with_fixint_encoding()
                .with_big_endian(),
            options,
            state: EncoderState::default(),
        };
        encoder.encode_header().unwrap();
        encoder
    }

    /// Create a new [`Encoder`] with the given compression scheme
    #[cfg(feature = "compression")]
    pub fn new_compressed(mut compression: CompressedOutput<W>, options: EncoderOptions) -> Self
    where
        Self: Sized,
    {
        compression.with_options(options);
        let mut encoder = Self {
            output: WriteCompressionEnum::CompressedOutput(compression),
            bincode: DefaultOptions::new()
                .with_fixint_encoding()
                .with_big_endian(),
            options,
            state: Default::default(),
        };
        encoder.encode_header().unwrap();
        encoder
    }

    /// Create a new [`Encoder`] with the given raw compression scheme
    pub fn new_raw(compression: RawOutput<W>, options: EncoderOptions) -> Self
    where
        Self: Sized,
    {
        let mut encoder = Self {
            output: WriteCompressionEnum::RawOutput(compression),
            bincode: DefaultOptions::new()
                .with_fixint_encoding()
                .with_big_endian(),
            options,
            state: Default::default(),
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
            SourceCamera::Dvs => U8,
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
    pub fn close_writer(self) -> Result<Option<W>, CodecError> {
        // self.output.byte_align()?;
        // self.write_eof()?;
        // self.flush_writer()?;
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

        // Encode the header extensions (for newer versions of the codec)
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

        self.bincode.serialize_into(
            &mut buffer,
            &EventStreamHeaderExtensionV3 {
                adu_interval: meta.adu_interval as u32,
            },
        )?;
        if meta.codec_version == 3 {
            return Ok(buffer);
        }
        Err(CodecError::BadFile)
    }

    /// Ingest an event
    #[inline(always)]
    pub fn ingest_event(&mut self, event: Event) -> Result<(), CodecError> {
        match self.options.event_drop {
            EventDrop::None => {}
            EventDrop::Manual {
                target_event_rate,
                alpha,
            } => {
                let now = Instant::now();
                let t_diff = now.duration_since(self.state.last_event_ts).as_secs_f64();
                let new_event_rate = alpha * self.state.current_event_rate + (1.0 - alpha) / t_diff;
                if new_event_rate > target_event_rate {
                    self.state.current_event_rate *= alpha;
                    return Ok(()); // skip this event
                }
                self.state.last_event_ts = now; // update time
                self.state.current_event_rate = new_event_rate;
            }
            EventDrop::Auto => {
                todo!()
            }
        }

        match self.options.event_order {
            EventOrder::Unchanged => self.output.ingest_event(event),
            EventOrder::Interleaved => {
                let dt = event.t;
                // First, push the event to the queue
                self.state.queue.push(event);

                let mut res = Ok(());
                if let Some(first_item_addr) = self.state.queue.peek() {
                    if first_item_addr.t < dt.saturating_sub(self.meta().delta_t_max) {
                        if let Some(first_item) = self.state.queue.pop() {
                            res = self.output.ingest_event(first_item);
                        }
                    }
                }
                res
            }
        }
    }
    // /// Ingest an event
    // #[cfg(feature = "compression")]
    // pub fn ingest_event_debug(&mut self, event: Event) -> Result<Option<Adu>, CodecError> {
    //     self.output.ingest_event_debug(event)
    // }

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

    pub fn get_options(&self) -> EncoderOptions {
        self.options
    }

    /// Keeps the compressed output options in sync with the encoder options. This prevents us
    /// from constantly having to look up a reference-counted variable, which is costly at this scale.
    pub fn sync_crf(&mut self) {
        match &mut self.output {
            #[cfg(feature = "compression")]
            WriteCompressionEnum::CompressedOutput(compressed_output) => {
                compressed_output.options = self.options;
            }
            WriteCompressionEnum::RawOutput(_) => {}
            WriteCompressionEnum::EmptyOutput(_) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::raw::stream::RawOutput;
    use crate::codec::{CodecMetadata, LATEST_CODEC_VERSION};
    use crate::{Coord, PlaneSize};
    use bitstream_io::{BigEndian, BitWriter};
    use std::io::BufWriter;
    use std::sync::{Arc, RwLock};

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
                adu_interval: 1,
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
            options: EncoderOptions::default(PlaneSize {
                width: 100,
                height: 100,
                channels: 1,
            }),
            state: EncoderState::default(),
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
                adu_interval: 1,
            },
            bufwriter,
        );
        let encoder = Encoder {
            output: WriteCompressionEnum::RawOutput(compression),
            bincode: DefaultOptions::new()
                .with_fixint_encoding()
                .with_big_endian(),
            options: EncoderOptions::default(PlaneSize {
                width: 100,
                height: 100,
                channels: 1,
            }),
            state: EncoderState::default(),
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
                adu_interval: 1,
            },
            bufwriter,
        );
        let mut encoder: Encoder<BufWriter<Vec<u8>>> = Encoder::new_raw(
            compression,
            EncoderOptions::default(PlaneSize {
                width: 1,
                height: 1,
                channels: 3,
            }),
        );

        let event = Event {
            coord: Coord {
                x: 0,
                y: 0,
                c: Some(0),
            },
            d: 0,
            t: 0,
        };

        encoder.ingest_event(event).unwrap();
        let mut writer = encoder.close_writer().unwrap().unwrap();
        writer.flush().unwrap();
        let output = writer.into_inner().unwrap();
        assert_eq!(output.len(), 37 + 22); // 37 bytes for the header, 22 bytes for the 2 events
    }

    #[test]
    #[cfg(feature = "compression")]
    fn compressed() {
        let output = Vec::new();
        let bufwriter = BufWriter::new(output);
        let (written_bytes_tx, written_bytes_rx) = std::sync::mpsc::channel();

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
                adu_interval: 1,
            },
            // frame: Default::default(),
            // adu: Adu::new(),
            // contexts: None,
            adu: Default::default(),
            stream: Some(Arc::new(RwLock::new(BitWriter::endian(
                bufwriter, BigEndian,
            )))),
            options: EncoderOptions::default(PlaneSize::default()),
            written_bytes_tx: Some(written_bytes_tx),
            last_message_sent: 0,
            last_message_written: Arc::new(RwLock::new(0)),
            _phantom: Default::default(),
        };
        let _encoder = Encoder {
            output: WriteCompressionEnum::CompressedOutput(compression),
            bincode: DefaultOptions::new()
                .with_fixint_encoding()
                .with_big_endian(),
            options: EncoderOptions::default(PlaneSize::default()),
            state: Default::default(),
        };
    }

    #[test]
    #[cfg(feature = "compression")]
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
                ref_interval: 255,
                delta_t_max: 255,
                event_size: 0,
                source_camera: Default::default(),
                adu_interval: Default::default(),
            },
            bufwriter,
        );
        let _encoder = Encoder {
            output: WriteCompressionEnum::CompressedOutput(compression),
            bincode: DefaultOptions::new()
                .with_fixint_encoding()
                .with_big_endian(),
            options: EncoderOptions::default(PlaneSize::default()),
            state: Default::default(),
        };
    }

    #[test]
    #[cfg(feature = "compression")]
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
                adu_interval: Default::default(),
            },
            bufwriter,
        );
        let _encoder =
            Encoder::new_compressed(compression, EncoderOptions::default(PlaneSize::default()));
    }
}
