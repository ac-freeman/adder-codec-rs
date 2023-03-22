use crate::codec::{CodecError, CodecMetadata, ReadCompression, ReadCompressionEnum};
use crate::SourceType::*;
use crate::{Event, PlaneSize, SourceCamera, SourceType};

use crate::codec::compressed::adu::frame::Adu;
use crate::codec::compressed::stream::CompressedInput;
use crate::codec::header::{
    EventStreamHeader, EventStreamHeaderExtensionV1, EventStreamHeaderExtensionV2,
};
use crate::codec::raw::stream::RawInput;
use crate::codec::CodecError::Deserialize;
use crate::SourceType::U8;
use bincode::config::{FixintEncoding, WithOtherEndian, WithOtherIntEncoding};
use bincode::{DefaultOptions, Options};
use bitstream_io::{BigEndian, BitRead, BitReader};
use std::io::{Read, Seek, SeekFrom};

/// Struct for decoding [`Event`]s from a stream
pub struct Decoder<R: Read + Seek> {
    input: ReadCompressionEnum<R>,
    bincode: WithOtherEndian<
        WithOtherIntEncoding<DefaultOptions, FixintEncoding>,
        bincode::config::BigEndian,
    >,
    _phantom: std::marker::PhantomData<R>,
}

#[allow(dead_code)]
impl<R: Read + Seek> Decoder<R> {
    /// Create a new decoder with the given compression scheme
    pub fn new_compressed(
        compression: CompressedInput<R>,
        reader: &mut BitReader<R, BigEndian>,
    ) -> Result<Self, CodecError>
    where
        Self: Sized,
    {
        let mut decoder = Self {
            input: ReadCompressionEnum::CompressedInput(compression),
            bincode: DefaultOptions::new()
                .with_fixint_encoding()
                .with_big_endian(),
            _phantom: std::marker::PhantomData,
        };
        decoder.decode_header(reader)?;
        Ok(decoder)
    }

    /// Create a new decoder with the given compression scheme
    pub fn new_raw(
        compression: RawInput<R>,
        reader: &mut BitReader<R, BigEndian>,
    ) -> Result<Self, CodecError>
    where
        Self: Sized,
    {
        let mut decoder = Self {
            input: ReadCompressionEnum::RawInput(compression),
            bincode: DefaultOptions::new()
                .with_fixint_encoding()
                .with_big_endian(),
            _phantom: std::marker::PhantomData,
        };
        decoder.decode_header(reader)?;
        Ok(decoder)
    }

    /// Returns a reference to the metadata of the underlying compression scheme
    #[inline]
    pub fn meta(&self) -> &CodecMetadata {
        self.input.meta()
    }

    /// Returns a mutable reference to the metadata of the underlying compression scheme
    #[inline]
    pub fn meta_mut(&mut self) -> &mut CodecMetadata {
        self.input.meta_mut()
    }

    /// Get the source data representation, based on the source camera
    #[allow(clippy::match_same_arms)]
    pub fn get_source_type(&self) -> SourceType {
        match self.input.meta().source_camera {
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

    /// Decode the header and its extensions
    fn decode_header(&mut self, reader: &mut BitReader<R, BigEndian>) -> Result<usize, CodecError> {
        let header_size = bincode::serialized_size(&EventStreamHeader::default())?;
        let mut buffer: Vec<u8> = vec![0; header_size as usize];
        reader.read_bytes(&mut buffer)?;

        let header = match self
            .bincode
            .deserialize_from::<_, EventStreamHeader>(&*buffer)
        {
            Ok(header) => header,
            Err(_) => return Err(Deserialize),
        };

        {
            if header.magic != self.input.magic() {
                return Err(CodecError::WrongMagic);
            }
            let meta = self.input.meta_mut();
            *meta = CodecMetadata {
                codec_version: header.version,
                header_size: header_size as usize,
                time_mode: Default::default(),
                plane: PlaneSize::new(header.width, header.height, header.channels)?,
                tps: header.tps,
                ref_interval: header.ref_interval,
                delta_t_max: header.delta_t_max,
                event_size: header.event_size,
                source_camera: Default::default(),
            };

            // Manual fix for malformed files with old software
            if meta.event_size == 10 {
                meta.event_size = 11;
            }
        }
        self.decode_header_extension(reader)?;
        Ok(self.input.meta().header_size)
    }

    fn decode_header_extension(
        &mut self,
        reader: &mut BitReader<R, BigEndian>,
    ) -> Result<(), CodecError> {
        let codec_version = self.input.meta().codec_version;
        if codec_version == 0 {
            return Ok(());
        }
        let mut extension_size =
            bincode::serialized_size(&EventStreamHeaderExtensionV1::default())?;
        let mut buffer: Vec<u8> = vec![0; extension_size as usize];
        reader.read_bytes(&mut buffer)?;
        let extension_v1 = match self
            .bincode
            .deserialize_from::<_, EventStreamHeaderExtensionV1>(&*buffer)
        {
            Ok(header) => header,
            Err(_) => return Err(Deserialize),
        };
        self.input.meta_mut().source_camera = extension_v1.source;
        self.input.meta_mut().header_size += extension_size as usize;

        if codec_version == 1 {
            return Ok(());
        }

        extension_size = bincode::serialized_size(&EventStreamHeaderExtensionV2::default())?;
        buffer = vec![0; extension_size as usize];
        reader.read_bytes(&mut buffer)?;
        let extension_v2 = match self
            .bincode
            .deserialize_from::<_, EventStreamHeaderExtensionV2>(&*buffer)
        {
            Ok(header) => header,
            Err(_) => return Err(Deserialize),
        };
        self.input.meta_mut().time_mode = extension_v2.time_mode;
        self.input.meta_mut().header_size += extension_size as usize;

        if codec_version == 2 {
            return Ok(());
        }

        Err(CodecError::UnsupportedVersion(codec_version))
    }

    /// Read and decode the next event from the input stream
    #[inline]
    pub fn digest_event(
        &mut self,
        reader: &mut BitReader<R, BigEndian>,
    ) -> Result<Event, CodecError> {
        self.input.digest_event(reader)
    }

    /// Read and decode the next event from the input stream
    #[inline]
    pub fn digest_event_debug(
        &mut self,
        reader: &mut BitReader<R, BigEndian>,
    ) -> Result<(Option<Adu>, Event), CodecError> {
        self.input.digest_event_debug(reader)
    }

    /// Sets the input stream position to the given absolute byte position
    pub fn set_input_stream_position(
        &mut self,
        reader: &mut BitReader<R, BigEndian>,
        position: u64,
    ) -> Result<(), CodecError> {
        self.input.set_input_stream_position(reader, position)
    }

    /// Returns the current position of the input stream in bytes
    pub fn get_input_stream_position(
        &self,
        reader: &mut BitReader<R, BigEndian>,
    ) -> Result<u64, CodecError> {
        Ok(reader.position_in_bits()? / 8)
    }

    /// Returns the EOF position, in bytes. This is the position of the first byte of the raw event
    /// which demarcates the end of the stream.
    pub fn get_eof_position(
        &mut self,
        reader: &mut BitReader<R, BigEndian>,
    ) -> Result<u64, CodecError> {
        for i in self.input.meta().event_size as i64..10 {
            reader.seek_bits(SeekFrom::End(i * 8))?;
            if let Err(CodecError::Eof) = self.digest_event(reader) {
                break;
            }
        }

        Ok(self.get_input_stream_position(reader)? - self.input.meta().event_size as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::compressed::stream::{CompressedInput, CompressedOutput};
    use crate::codec::encoder::Encoder;
    use crate::codec::raw::stream::{RawInput, RawOutput};

    use crate::Coord;
    use std::io::{BufReader, BufWriter, Cursor, Write};

    fn stock_event() -> Event {
        Event {
            coord: Coord {
                x: 0,
                y: 0,
                c: None,
            },
            d: 0,
            delta_t: 0,
        }
    }

    fn setup_encoded_raw(codec_version: u8) -> Vec<u8> {
        let output = Vec::new();

        let bufwriter = BufWriter::new(output);
        let compression = RawOutput::new(
            CodecMetadata {
                codec_version,
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
        let mut encoder: Encoder<BufWriter<Vec<u8>>> = Encoder::new_raw(compression);

        let event = stock_event();
        encoder.ingest_event(event).unwrap();
        let mut writer = encoder.close_writer().unwrap().unwrap();

        writer.flush().unwrap();

        writer.into_inner().unwrap()
    }

    fn setup_encoded_compressed(codec_version: u8) -> Vec<u8> {
        let output = Vec::new();

        let bufwriter = BufWriter::new(output);
        let compression = CompressedOutput::new(
            CodecMetadata {
                codec_version,
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
        let encoder: Encoder<BufWriter<Vec<u8>>> = Encoder::new_compressed(compression);

        // let event = stock_event();
        //
        // encoder.ingest_event(&event).unwrap();
        let mut writer = encoder.close_writer().unwrap().unwrap();
        writer.flush().unwrap();

        writer.into_inner().unwrap()
    }

    #[test]
    fn header_v0_raw() {
        let output = setup_encoded_raw(0);
        let tmp = Cursor::new(&*output);

        let bufreader = BufReader::new(tmp);

        let compression = RawInput::new();

        let mut bitreader = BitReader::endian(bufreader, BigEndian);
        let reader = Decoder::new_raw(compression, &mut bitreader).unwrap();
        assert_eq!(reader.input.meta().header_size, 25);
    }

    #[test]
    fn header_v1_raw() {
        let output = setup_encoded_raw(1);
        let tmp = Cursor::new(&*output);
        let bufreader = BufReader::new(tmp);
        let compression = RawInput::new();

        let mut bitreader = BitReader::endian(bufreader, BigEndian);
        let reader = Decoder::new_raw(compression, &mut bitreader).unwrap();
        assert_eq!(reader.input.meta().header_size, 29);
    }

    #[test]
    fn header_v2_raw() {
        let output = setup_encoded_raw(2);
        let tmp = Cursor::new(&*output);
        let bufreader = BufReader::new(tmp);
        let compression = RawInput::new();

        let mut bitreader = BitReader::endian(bufreader, BigEndian);
        let reader = Decoder::new_raw(compression, &mut bitreader).unwrap();
        assert_eq!(reader.input.meta().header_size, 33);
    }

    #[test]
    fn header_v0_compressed() {
        let output = setup_encoded_compressed(0);
        let tmp = Cursor::new(&*output);
        let bufreader = BufReader::new(tmp);
        let compression = CompressedInput::new(255, 255);

        let mut bitreader = BitReader::endian(bufreader, BigEndian);
        let reader = Decoder::new_compressed(compression, &mut bitreader).unwrap();
        assert_eq!(reader.input.meta().header_size, 25);
    }

    #[test]
    fn header_v1_compressed() {
        let output = setup_encoded_compressed(1);
        let tmp = Cursor::new(&*output);
        let bufreader = BufReader::new(tmp);
        let compression = CompressedInput::new(255, 255);

        let mut bitreader = BitReader::endian(bufreader, BigEndian);
        let reader = Decoder::new_compressed(compression, &mut bitreader).unwrap();
        assert_eq!(reader.input.meta().header_size, 29);
    }

    #[test]
    fn header_v2_compressed() {
        let output = setup_encoded_compressed(2);
        let tmp = Cursor::new(&*output);
        let bufreader = BufReader::new(tmp);
        let compression = CompressedInput::new(255, 255);

        let mut bitreader = BitReader::endian(bufreader, BigEndian);
        let reader = Decoder::new_compressed(compression, &mut bitreader).unwrap();
        assert_eq!(reader.input.meta().header_size, 33);
    }

    #[test]
    fn digest_event_raw() {
        let output = setup_encoded_raw(2);
        let tmp = Cursor::new(&*output);
        let bufreader = BufReader::new(tmp);
        let compression = RawInput::new();

        let mut bitreader = BitReader::endian(bufreader, BigEndian);
        let mut reader = Decoder::new_raw(compression, &mut bitreader).unwrap();
        let event = reader.digest_event(&mut bitreader).unwrap();
        assert_eq!(event, stock_event());
    }
}
