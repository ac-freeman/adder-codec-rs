use crate::codec::{CodecError, CodecMetadata, ReadCompression};
use crate::SourceType::*;
use crate::{Event, PlaneSize, SourceCamera, SourceType, EOF_EVENT};

use std::io::{Read, Seek, SeekFrom};

use crate::codec::header::{
    EventStreamHeader, EventStreamHeaderExtensionV1, EventStreamHeaderExtensionV2,
};
use crate::codec::CodecError::Deserialize;
use crate::SourceType::U8;
use bincode::config::{FixintEncoding, WithOtherEndian, WithOtherIntEncoding};
use bincode::{DefaultOptions, Options};
use bitstream_io::{BigEndian, BitRead, BitReader};

pub struct Decoder<R> {
    compression: Box<dyn ReadCompression<R>>,
    bincode: WithOtherEndian<
        WithOtherIntEncoding<DefaultOptions, FixintEncoding>,
        bincode::config::BigEndian,
    >,
}

#[allow(dead_code)]
impl<R: Read + Seek> Decoder<R> {
    pub fn new(
        compression: Box<dyn ReadCompression<R>>,
        reader: &mut BitReader<R, BigEndian>,
    ) -> Result<Self, CodecError>
    where
        Self: Sized,
    {
        let mut decoder = Self {
            compression,
            bincode: DefaultOptions::new()
                .with_fixint_encoding()
                .with_big_endian(),
        };
        decoder.decode_header(reader)?;
        Ok(decoder)
    }

    #[inline]
    pub fn meta(&self) -> &CodecMetadata {
        self.compression.meta()
    }

    #[inline]
    pub fn meta_mut(&mut self) -> &mut CodecMetadata {
        self.compression.meta_mut()
    }

    #[allow(clippy::match_same_arms)]
    pub fn get_source_type(&self) -> SourceType {
        match self.compression.meta().source_camera {
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

    fn decode_header(&mut self, reader: &mut BitReader<R, BigEndian>) -> Result<usize, CodecError> {
        let header_size = bincode::serialized_size(&EventStreamHeader::default())?;
        let mut buffer: Vec<u8> = vec![0; header_size as usize];
        reader.read_bytes(&mut buffer)?;

        let header = match self
            .bincode
            .deserialize_from::<_, EventStreamHeader>(&*buffer)
        {
            Ok(header) => header,
            Err(_) => return Err(Deserialize.into()),
        };

        {
            if header.magic != self.compression.magic() {
                return Err(CodecError::WrongMagic);
            }
            let meta = self.compression.meta_mut();
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
        }
        self.decode_header_extension(reader)?;
        Ok(self.compression.meta().header_size)

        // match &mut self.input_stream {
        //     None => Err(Error::UnitializedStream.into()),
        //     Some(stream) => {
        //         let header = match self
        //             .bincode
        //             .deserialize_from::<_, EventStreamHeader>(stream.get_mut())
        //         {
        //             Ok(header) => header,
        //             Err(_) => return Err(Deserialize.into()),
        //         };
        //
        //         self.codec_version = header.version;
        //
        //         self.plane = match PlaneSize::new(header.width, header.height, header.channels) {
        //             Ok(a) => a,
        //             Err(_) => {
        //                 return Err(Error::BadFile.into());
        //             }
        //         };
        //
        //         self.tps = header.tps;
        //         self.ref_interval = header.ref_interval;
        //         self.delta_t_max = header.delta_t_max;
        //         self.event_size = header.event_size;
        //
        //         match header.magic {
        //             MAGIC_RAW => {}
        //             _ => return Err(Error::BadFile.into()),
        //         };
        //
        //         decode_header_extension(self)?;
        //         self.header_size = self.get_input_stream_position()? as usize;
        //
        //         Ok(self.header_size)
        //     }
        // }
    }

    fn decode_header_extension(
        &mut self,
        reader: &mut BitReader<R, BigEndian>,
    ) -> Result<(), CodecError> {
        let codec_version = self.compression.meta().codec_version;
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
            Err(_) => return Err(Deserialize.into()),
        };
        self.compression.meta_mut().source_camera = extension_v1.source;
        self.compression.meta_mut().header_size += extension_size as usize;

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
            Err(_) => return Err(Deserialize.into()),
        };
        self.compression.meta_mut().time_mode = extension_v2.time_mode;
        self.compression.meta_mut().header_size += extension_size as usize;

        if codec_version == 2 {
            return Ok(());
        }

        Err(CodecError::UnsupportedVersion(codec_version).into())
    }

    #[inline]
    pub fn digest_event(
        &mut self,
        reader: &mut BitReader<R, BigEndian>,
    ) -> Result<Event, CodecError> {
        self.compression.digest_event(reader)
    }

    pub fn set_input_stream_position(
        &mut self,
        reader: &mut BitReader<R, BigEndian>,
        position: u64,
    ) -> Result<(), CodecError> {
        self.compression.set_input_stream_position(reader, position)
    }

    /// Returns the current position of the input stream in bytes
    pub fn get_input_stream_position(
        &self,
        reader: &mut BitReader<R, BigEndian>,
    ) -> Result<u64, CodecError> {
        Ok(reader.position_in_bits()? / 8)
    }

    pub fn get_eof_position(
        &mut self,
        reader: &mut BitReader<R, BigEndian>,
    ) -> Result<u64, CodecError> {
        for i in self.compression.meta().event_size as i64..10 {
            let pos = reader.seek_bits(SeekFrom::End(i * 8))?;
            match self.digest_event(reader) {
                Err(CodecError::Eof) => {
                    break;
                }
                _ => {}
            }
        }

        Ok(self.get_input_stream_position(reader)? - self.compression.meta().event_size as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::compressed::stream::{CompressedInput, CompressedOutput};
    use crate::codec::encoder::Encoder;
    use crate::codec::raw::stream::{RawInput, RawOutput};
    use crate::codec::WriteCompression;

    use crate::Coord;
    use std::io::{BufReader, BufWriter, Write};

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
        let compression = <RawOutput<_> as WriteCompression<BufWriter<Vec<u8>>>>::new(
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
        let mut encoder: Encoder<BufWriter<Vec<u8>>> = Encoder::new(Box::new(compression));

        let event = stock_event();
        encoder.ingest_event(&event).unwrap();
        let mut writer = encoder.close_writer().unwrap();
        writer.flush().unwrap();

        writer.into_inner().unwrap()
    }

    fn setup_encoded_compressed(codec_version: u8) -> Vec<u8> {
        let output = Vec::new();

        let bufwriter = BufWriter::new(output);
        let compression = <CompressedOutput<_> as WriteCompression<BufWriter<Vec<u8>>>>::new(
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
        let encoder: Encoder<BufWriter<Vec<u8>>> = Encoder::new(Box::new(compression));

        // let event = stock_event();
        //
        // encoder.ingest_event(&event).unwrap();
        let mut writer = encoder.close_writer().unwrap();
        writer.flush().unwrap();

        writer.into_inner().unwrap()
    }

    #[test]
    fn header_v0_raw() {
        let output = setup_encoded_raw(0);
        let tmp = &*output;

        let bufreader = BufReader::new(tmp);

        let mut compression = <RawInput as ReadCompression<BufReader<&[u8]>>>::new();

        let mut bitreader = BitReader::endian(bufreader, BigEndian);
        let reader = Decoder::new(Box::new(compression), &mut bitreader);
        assert_eq!(reader.compression.meta().header_size, 25);
    }

    #[test]
    fn header_v1_raw() {
        let output = setup_encoded_raw(1);
        let tmp = &*output;

        let bufreader = BufReader::new(tmp);
        let mut compression = <RawInput as ReadCompression<BufReader<&[u8]>>>::new();

        let mut bitreader = BitReader::endian(bufreader, BigEndian);
        let reader = Decoder::new(Box::new(compression), &mut bitreader);
        assert_eq!(reader.compression.meta().header_size, 29);
    }

    #[test]
    fn header_v2_raw() {
        let output = setup_encoded_raw(2);
        let tmp = &*output;

        let bufreader = BufReader::new(tmp);
        let mut compression = <RawInput as ReadCompression<BufReader<&[u8]>>>::new();

        let mut bitreader = BitReader::endian(bufreader, BigEndian);
        let reader = Decoder::new(Box::new(compression), &mut bitreader);
        assert_eq!(reader.compression.meta().header_size, 33);
    }

    #[test]
    fn header_v0_compressed() {
        let output = setup_encoded_compressed(0);
        let tmp = &*output;

        let bufreader = BufReader::new(tmp);
        let mut compression = <CompressedInput as ReadCompression<BufReader<&[u8]>>>::new();

        let mut bitreader = BitReader::endian(bufreader, BigEndian);
        let reader = Decoder::new(Box::new(compression), &mut bitreader);
        assert_eq!(reader.compression.meta().header_size, 25);
    }

    #[test]
    fn header_v1_compressed() {
        let output = setup_encoded_compressed(1);
        let tmp = &*output;

        let bufreader = BufReader::new(tmp);
        let mut compression = <CompressedInput as ReadCompression<BufReader<&[u8]>>>::new();

        let mut bitreader = BitReader::endian(bufreader, BigEndian);
        let reader = Decoder::new(Box::new(compression), &mut bitreader);
        assert_eq!(reader.compression.meta().header_size, 29);
    }

    #[test]
    fn header_v2_compressed() {
        let output = setup_encoded_compressed(2);
        let tmp = &*output;

        let bufreader = BufReader::new(tmp);
        let mut compression = <CompressedInput as ReadCompression<BufReader<&[u8]>>>::new();

        let mut bitreader = BitReader::endian(bufreader, BigEndian);
        let reader = Decoder::new(Box::new(compression), &mut bitreader);
        assert_eq!(reader.compression.meta().header_size, 33);
    }

    #[test]
    fn digest_event_raw() {
        let output = setup_encoded_raw(2);
        let tmp = &*output;

        let bufreader = BufReader::new(tmp);
        let mut compression = <RawInput as ReadCompression<BufReader<&[u8]>>>::new();

        let mut bitreader = BitReader::endian(bufreader, BigEndian);
        let mut reader = Decoder::new(Box::new(compression), &mut bitreader);
        let event = reader.digest_event(&mut bitreader).unwrap();
        assert_eq!(event, stock_event());
    }
}
