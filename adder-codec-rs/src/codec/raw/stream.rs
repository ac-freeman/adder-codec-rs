use crate::header::{
    EventStreamHeaderExtensionV0, EventStreamHeaderExtensionV1, EventStreamHeaderExtensionV2,
    MAGIC_RAW,
};
use crate::SourceType::{F32, F64, U16, U32, U64, U8};

use crate::codec::raw::stream::Error::{Deserialize, Eof};
use crate::codec::units::avu::{Avu, Type};
use crate::codec::Codec;
use crate::{
    Coord, DeltaT, Event, EventSingle, EventStreamHeader, PlaneSize, SourceCamera, SourceType,
    TimeMode, EOF_PX_ADDRESS,
};
use bincode::config::{BigEndian, FixintEncoding, WithOtherEndian, WithOtherIntEncoding};
use bincode::{DefaultOptions, Options};
use std::fs::File;
use std::io::{BufReader, BufWriter, Seek, SeekFrom, Write};
use std::{fmt, io, mem};

pub const LATEST_CODEC_VERSION: u8 = 2;

#[derive(Debug)]
pub enum Error {
    /// Stream has not been initialized
    UnitializedStream,

    /// Reached end of file when expected
    Eof,

    /// Could not deserialize data. EOF reached at unexpected time.
    Deserialize,

    /// File formatted incorrectly
    BadFile,

    /// Attempted to seek to a bad position in the stream
    Seek,

    /// Bincode error
    BincodeError(bincode::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        // write!(f, "Stream error")
        write!(f, "{self:?}")
    }
}

impl From<Error> for Box<dyn std::error::Error> {
    fn from(value: Error) -> Self {
        value.to_string().into()
    }
}

impl From<Box<bincode::ErrorKind>> for Error {
    fn from(value: Box<bincode::ErrorKind>) -> Self {
        Error::BincodeError(value)
    }
}

pub struct Raw {
    output_stream: Option<BufWriter<File>>,
    input_stream: Option<BufReader<File>>,
    pub codec_version: u8,
    pub header_size: usize,
    pub time_mode: TimeMode,
    pub plane: PlaneSize,
    pub tps: DeltaT,
    pub ref_interval: DeltaT,
    pub delta_t_max: DeltaT,
    pub event_size: u8,
    pub source_camera: SourceCamera,
    avu: Avu,
    bincode: WithOtherEndian<WithOtherIntEncoding<DefaultOptions, FixintEncoding>, BigEndian>,
}

impl Codec for Raw {
    fn new() -> Self {
        Raw {
            output_stream: None,
            input_stream: None,
            codec_version: LATEST_CODEC_VERSION,
            header_size: 0,
            time_mode: TimeMode::DeltaT,
            plane: PlaneSize::default(),
            tps: 0,
            ref_interval: 0,
            delta_t_max: 0,
            event_size: 0,
            source_camera: SourceCamera::default(),
            avu: Avu::default(),
            bincode: DefaultOptions::new()
                .with_fixint_encoding()
                .with_big_endian(),
        }
    }

    #[allow(clippy::match_same_arms)]
    fn get_source_type(&self) -> SourceType {
        match self.source_camera {
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

    fn write_eof(&mut self) -> Result<(), Error> {
        match &mut self.output_stream {
            None => {
                // panic!("Output stream not initialized");
            }
            Some(_stream) => {
                let eof = Event {
                    coord: Coord {
                        x: EOF_PX_ADDRESS,
                        y: EOF_PX_ADDRESS,
                        c: Some(0),
                    },
                    d: 0,
                    delta_t: 0,
                };
                self.encode_event(&eof)?;
            }
        };
        Ok(())
    }
    fn flush_writer(&mut self) -> io::Result<()> {
        match &mut self.output_stream {
            None => Ok(()),
            Some(stream) => Ok(stream.flush()?),
        }
    }

    fn close_writer(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.write_eof()?;
        match &mut self.output_stream {
            None => {}
            Some(stream) => {
                stream.flush()?;
            }
        }
        let mut tmp = None;
        mem::swap(&mut tmp, &mut self.output_stream);
        drop(tmp);
        Ok(())
    }

    fn close_reader(&mut self) {
        let mut tmp = None;
        mem::swap(&mut tmp, &mut self.input_stream);
        drop(tmp);
    }

    fn set_output_stream(&mut self, stream: Option<BufWriter<File>>) {
        self.output_stream = stream;
    }

    fn has_output_stream(&self) -> bool {
        self.output_stream.is_some()
    }

    fn set_input_stream(&mut self, stream: Option<BufReader<File>>) {
        self.input_stream = stream;
    }

    fn set_input_stream_position(&mut self, pos: u64) -> Result<(), Error> {
        if (pos - self.header_size as u64) % u64::from(self.event_size) != 0 {
            eprintln!("Attempted to seek to bad position in stream: {pos}");
            return Err(Error::Seek);
        }
        match &mut self.input_stream {
            None => {
                return Err(Error::UnitializedStream);
            }
            Some(stream) => match stream.seek(SeekFrom::Start(pos)) {
                Ok(_) => {}
                Err(_) => return Err(Error::Seek),
            },
        };
        Ok(())
    }

    fn set_input_stream_position_from_end(&mut self, mut pos: i64) -> Result<(), Error> {
        if pos > 0 {
            pos = -pos;
        }
        // TODO: check that the seek position is event-aligned
        match &mut self.input_stream {
            None => {
                return Err(Error::UnitializedStream);
            }
            Some(stream) => match stream.seek(SeekFrom::End(pos)) {
                Ok(_) => {}
                Err(_) => return Err(Error::Seek),
            },
        };
        Ok(())
    }

    fn get_input_stream_position(&mut self) -> Result<u64, Box<dyn std::error::Error>> {
        match &mut self.input_stream {
            None => Err(Error::UnitializedStream.into()),
            Some(stream) => Ok(stream.stream_position()?),
        }
    }

    fn get_output_stream_position(&mut self) -> Result<u64, Box<dyn std::error::Error>> {
        match &mut self.output_stream {
            None => Err(Error::UnitializedStream.into()),
            Some(stream) => Ok(stream.stream_position()?),
        }
    }

    // TODO: return more relevant errors
    fn get_eof_position(&mut self) -> Result<u64, Box<dyn std::error::Error>> {
        match &mut self.input_stream {
            None => {
                return Err(Error::UnitializedStream.into());
            }
            Some(stream) => stream.seek(SeekFrom::End(-i64::from(self.event_size)))?,
        };

        for _ in 0..10 {
            match self.decode_event() {
                Err(Eof) => {
                    let stream = self.input_stream.as_mut().ok_or(Error::UnitializedStream)?;
                    return Ok(stream.stream_position()? - u64::from(self.event_size));
                }
                Err(Deserialize) => break,
                _ => {}
            }

            let stream = self.input_stream.as_mut().ok_or(Error::UnitializedStream)?;

            // Keep iterating back, searching for the Eof
            match stream.seek(SeekFrom::Current(-(i64::from(self.event_size) * 2))) {
                Ok(_) => {}
                Err(_) => break,
            };
        }

        self.set_input_stream_position_from_end(0)?;
        self.get_input_stream_position()
    }

    /// Encode the header for this [`Raw`]. If an [`input_stream`] is open for this struct
    /// already, then it is dropped. Intended usage is to create a separate [`Raw`] if you
    /// want to read and write two streams at once (for example, if you are cropping the spatial
    /// pixels of a stream, reducing the number of channels, or scaling the [`DeltaT`] values in
    /// some way).
    fn encode_header(
        &mut self,
        plane: PlaneSize,
        tps: DeltaT,
        ref_interval: DeltaT,
        delta_t_max: DeltaT,
        codec_version: u8,
        source_camera: Option<SourceCamera>,
        time_mode: Option<TimeMode>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.plane = plane.clone();
        self.tps = tps;
        self.ref_interval = ref_interval;
        self.delta_t_max = delta_t_max;
        self.codec_version = codec_version;
        let header = EventStreamHeader::new(
            MAGIC_RAW,
            plane,
            tps,
            ref_interval,
            delta_t_max,
            codec_version,
        );
        assert_eq!(header.magic, MAGIC_RAW);

        self.input_stream = None;
        self.source_camera = source_camera.unwrap_or_default();
        self.time_mode = time_mode.unwrap_or_default();

        encode_header_extension(self, header, source_camera, time_mode)?;
        self.header_size = self.get_output_stream_position()? as usize;
        Ok(())
    }

    fn decode_header(&mut self) -> Result<usize, Box<dyn std::error::Error>> {
        match &mut self.input_stream {
            None => Err(Error::UnitializedStream.into()),
            Some(stream) => {
                let header = match self
                    .bincode
                    .deserialize_from::<_, EventStreamHeader>(stream.get_mut())
                {
                    Ok(header) => header,
                    Err(_) => return Err(Deserialize.into()),
                };

                self.codec_version = header.version;

                self.plane = match PlaneSize::new(header.width, header.height, header.channels) {
                    Ok(a) => a,
                    Err(_) => {
                        return Err(Error::BadFile.into());
                    }
                };

                self.tps = header.tps;
                self.ref_interval = header.ref_interval;
                self.delta_t_max = header.delta_t_max;
                self.event_size = header.event_size;

                match header.magic {
                    MAGIC_RAW => {}
                    _ => return Err(Error::BadFile.into()),
                };

                decode_header_extension(self)?;
                self.header_size = self.get_input_stream_position()? as usize;

                Ok(self.header_size)
            }
        }
    }

    fn encode_event(&mut self, event: &Event) -> Result<(), Error> {
        if self.codec_version >= 3 {
            return self.encode_event_v3(event);
        }

        match &mut self.output_stream {
            None => Err(Error::UnitializedStream),
            Some(stream) => {
                // NOTE: for speed, the following checks only run in debug builds. It's entirely
                // possibly to encode nonsensical events if you want to.
                debug_assert!(event.coord.x < self.plane.width || event.coord.x == EOF_PX_ADDRESS);
                debug_assert!(event.coord.y < self.plane.height || event.coord.y == EOF_PX_ADDRESS);
                let output_event: EventSingle;
                if self.plane.channels == 1 {
                    output_event = event.into();
                    self.bincode.serialize_into(&mut *stream, &output_event)?;
                    // bincode::serialize_into(&mut *stream, &output_event, my_options).unwrap();
                } else {
                    self.bincode.serialize_into(&mut *stream, event)?;
                }
                Ok(())
            }
        }
    }

    fn encode_event_v3(&mut self, event: &Event) -> Result<(), Error> {
        if self.avu.header.size == 0 {
            self.avu.header.time = event.delta_t as u64;
            self.avu.header.avu_type = Type::AbsEvents
        }

        self.avu.data.append(&mut bincode::serialize(&event)?);
        self.avu.header.size += mem::size_of::<Event>() as u64;

        if self.avu.data.len() >= 4000 {
            // TODO: temporary fixed value
            self.flush_avu()?;
        }
        Ok(())
    }

    fn encode_events(&mut self, events: &[Event]) -> Result<(), Error> {
        for event in events {
            self.encode_event(event)?;
        }
        Ok(())
    }

    fn encode_events_events(&mut self, events: &[Vec<Event>]) -> Result<(), Error> {
        for v in events {
            self.encode_events(v)?;
        }
        Ok(())
    }

    fn decode_event(&mut self) -> Result<Event, Error> {
        // let mut buf = vec![0u8; self.event_size as usize];
        let event: Event = match &mut self.input_stream {
            None => return Err(Error::UnitializedStream),
            Some(stream) => {
                if self.plane.channels == 1 {
                    match self.bincode.deserialize_from::<_, EventSingle>(stream) {
                        Ok(ev) => ev.into(),
                        Err(_e) => return Err(Deserialize),
                    }
                } else {
                    match self.bincode.deserialize_from(stream) {
                        Ok(ev) => ev,
                        Err(_) => return Err(Deserialize),
                    }
                }
            }
        };
        if event.coord.y == EOF_PX_ADDRESS && event.coord.x == EOF_PX_ADDRESS {
            return Err(Eof);
        }
        Ok(event)
    }

    fn flush_avu(&mut self) -> Result<(), Error> {
        match &mut self.output_stream {
            None => Err(Error::UnitializedStream),
            Some(stream) => {
                self.bincode.serialize_into(&mut *stream, &self.avu)?;
                Ok(())
            }
        }
    }
}

fn encode_header_extension(
    raw: &mut Raw,
    header: EventStreamHeader,
    source_camera: Option<SourceCamera>,
    time_mode: Option<TimeMode>,
) -> Result<(), Box<dyn std::error::Error>> {
    match &mut raw.output_stream {
        None => Err(Error::UnitializedStream.into()),
        Some(stream) => {
            raw.bincode.serialize_into(&mut *stream, &header)?;

            raw.bincode
                .serialize_into(&mut *stream, &EventStreamHeaderExtensionV0 {})?;

            if raw.codec_version == 0 {
                return Ok(());
            }
            raw.bincode.serialize_into(
                &mut *stream,
                &EventStreamHeaderExtensionV1 {
                    source: source_camera.expect("source_camera must be set for codec version 1"),
                },
            )?;
            if raw.codec_version == 1 {
                return Ok(());
            }

            raw.bincode.serialize_into(
                &mut *stream,
                &EventStreamHeaderExtensionV2 {
                    time_mode: time_mode.expect("time_mode must be set for codec version 2"),
                },
            )?;
            if raw.codec_version == 2 {
                return Ok(());
            }
            Err(Error::BadFile.into())
        }
    }
}

fn decode_header_extension(raw: &mut Raw) -> Result<(), Box<dyn std::error::Error>> {
    match &mut raw.input_stream {
        None => Err(Error::UnitializedStream.into()),
        Some(stream) => {
            if raw.codec_version == 0 {
                // Leave source camera the default (FramedU8)
                return Ok(());
            }
            raw.source_camera = raw
                .bincode
                .deserialize_from::<_, EventStreamHeaderExtensionV1>(stream.get_mut())?
                .source;
            if raw.codec_version == 1 {
                return Ok(());
            }

            raw.time_mode = raw
                .bincode
                .deserialize_from::<_, EventStreamHeaderExtensionV2>(stream.get_mut())?
                .time_mode;
            if raw.codec_version == 2 {
                return Ok(());
            }
            Err(Error::BadFile.into())
        }
    }
}

fn _mem_size_word_aligned(size: usize) -> usize {
    let mut size = size;
    if size % 4 != 0 {
        size += 4 - (size % 4);
    }
    size
}

#[cfg(test)]
mod tests {
    use crate::codec::raw::stream::Raw;
    use crate::codec::Codec;
    use crate::SourceCamera::FramedU8;
    use crate::{Coord, Event, PlaneSize, TimeMode};
    use rand::Rng;
    use std::fs;

    #[test]
    fn ttt() {
        let n: u32 = rand::thread_rng().gen();
        let mut stream: Raw = Codec::new();
        stream
            .open_writer("./TEST_".to_owned() + n.to_string().as_str() + ".addr")
            .expect("Couldn't open file");
        let plane = PlaneSize {
            width: 50,
            height: 50,
            channels: 1,
        };
        stream
            .encode_header(
                plane,
                53000,
                4000,
                50000,
                1,
                Some(FramedU8),
                Some(TimeMode::DeltaT),
            )
            .unwrap();
        let event: Event = Event {
            coord: Coord {
                x: 10,
                y: 30,
                c: None,
            },
            d: 5,
            delta_t: 1000,
        };
        stream.encode_event(&event).unwrap();
        stream.flush_writer().unwrap();
        stream
            .open_reader("./TEST_".to_owned() + n.to_string().as_str() + ".addr")
            .expect("Couldn't open file");
        stream.decode_header().unwrap();
        let res = stream.decode_event();
        match res {
            Ok(decoded_event) => {
                assert_eq!(event, decoded_event);
            }
            Err(_) => {
                panic!("Couldn't decode event")
            }
        }

        let plane = PlaneSize {
            width: 20,
            height: 30,
            channels: 3,
        };
        stream
            .encode_header(
                plane,
                473_289,
                477_893,
                4_732_987,
                1,
                Some(FramedU8),
                Some(TimeMode::DeltaT),
            )
            .unwrap();
        assert!(stream.input_stream.is_none());

        stream.close_writer().unwrap();
        fs::remove_file("./TEST_".to_owned() + n.to_string().as_str() + ".addr").unwrap();
        // Don't check the error
    }
}
