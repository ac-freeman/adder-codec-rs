use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::mem;
use bincode::config::{BigEndian, FixintEncoding, WithOtherEndian, WithOtherIntEncoding};
use bytes::{Buf, Bytes};
use crate::{Codec, Coord, DeltaT, Event, EventCoordless, EventSingle, EventStreamHeader};
use crate::header::MAGIC_RAW;
use bytes::BytesMut;
use bytes::BufMut;
use bincode::{DefaultOptions, Options};

pub struct RawStream {
    output_stream: Option<BufWriter<File>>,
    input_stream: Option<BufReader<File>>,
    pub width: u16,
    pub height: u16,
    pub tps: DeltaT,
    pub ref_interval: DeltaT,
    pub delta_t_max: DeltaT,
    pub channels: u8,
    event_size: u8,
    bincode: WithOtherEndian<WithOtherIntEncoding<DefaultOptions, FixintEncoding>, BigEndian>,
}

impl Codec for RawStream {
    fn new() -> Self {
        RawStream {
            output_stream: None,
            input_stream: None,
            width: 0,
            height: 0,
            tps: 0,
            ref_interval: 0,
            delta_t_max: 0,
            channels: 0,
            event_size: 0,
            bincode: bincode::DefaultOptions::new()
                    .with_fixint_encoding()
                    .with_big_endian(),
        }
    }

    fn flush_writer(&mut self) {
        match &mut self.output_stream {
            None => {}
            Some(stream) => {
                stream.flush().unwrap();
            }
        }
    }

    fn close_writer(&mut self) {
        match &mut self.output_stream {
            None => {}
            Some(stream) => {
                stream.flush().unwrap();
            }
        }
        let mut tmp = None;
        mem::swap(&mut tmp, &mut self.output_stream);
        drop(tmp);
    }

    fn close_reader(&mut self) {
        let mut tmp = None;
        mem::swap(&mut tmp, &mut self.input_stream);
        drop(tmp);
    }

    fn set_output_stream(&mut self, stream: Option<BufWriter<File>>) {
        self.output_stream = stream;
    }

    fn set_input_stream(&mut self, stream: Option<BufReader<File>>) {
        self.input_stream = stream;
    }

    /// Encode the header for this [RawStream]. If an [input_stream] is open for this struct
    /// already, then it is dropped. Intended usage is to create a separate [RawStream] if you
    /// want to read and write two streams at once (for example, if you are cropping the spatial
    /// pixels of a stream, reducing the number of channels, or scaling the [DeltaT] values in
    /// some way).
    fn encode_header(&mut self,
                     width: u16,
                     height: u16,
                     tps: DeltaT,
                     ref_interval: DeltaT,
                     delta_t_max: DeltaT,
                     channels: u8) {
        self.width = width;
        self.height = height;
        self.tps = tps;
        self.ref_interval = ref_interval;
        self.delta_t_max = delta_t_max;
        self.channels = channels;
        let header = EventStreamHeader::new(MAGIC_RAW, width, height, tps, ref_interval, delta_t_max, channels);
        assert_eq!(header.magic, MAGIC_RAW);
        match &mut self.output_stream {
            None => {
                panic!("Output stream not initialized");
            }
            Some(stream) => {
                stream.write_all(&Bytes::from(&header).to_vec())
                    .expect("Unable to write header");
            }
        }
        self.input_stream = None;
    }

    fn decode_header(&mut self) {
        match &mut self.input_stream {
            None => {
                panic!("Output stream not initialized");
            }
            Some(stream) => {
                let header = EventStreamHeader::read_header(stream);
                self.width = header.width;
                self.height = header.height;
                self.tps = header.tps;
                self.ref_interval = header.ref_interval;
                self.delta_t_max = header.delta_t_max;
                self.channels = header.channels;
                self.event_size = header.event_size;
                assert_eq!(header.magic, MAGIC_RAW);
            }
        }

    }

    fn encode_event(&mut self, event: &Event) {
        match &mut self.output_stream {
            None => {
                panic!("Output stream not initialized");
            }
            Some(stream) => {
                // NOTE: for speed, the following checks only run in debug builds. It's entirely
                // possibly to encode non-sensical events if you want to.
                debug_assert!(event.coord.x < self.width);
                debug_assert!(event.coord.y < self.height);
                match event.coord.c {
                    None => {
                        debug_assert_eq!(self.channels, 1);
                    }
                    Some(c) => {
                        debug_assert!(c > 0);
                        debug_assert!(c <= self.channels);
                        if c == 1 {
                            debug_assert!(self.channels > 1);
                        }
                    }
                }
                debug_assert!(event.delta_t <= self.delta_t_max);
                stream.write_all(&Bytes::from(event).to_vec())
                    .expect("Unable to write event");
            }
        }
    }

    fn encode_events_events(&mut self, events: &Vec<Vec<Event>>) {
        for v in events {
            self.encode_events(v);
        }
    }

    fn encode_events(&mut self, events: &Vec<Event>) {
        match &mut self.output_stream {
            None => {
                panic!("Output stream not initialized");
            }
            Some(stream) => {
                // bincode::encode_into_writer(events, stream, my_options);


                // let mut out_buf = BytesMut::with_capacity(events.len() * core::mem::size_of::<Event>());
                //
                let mut output_event = EventSingle {
                    coord: Default::default(),
                    d: 0,
                    delta_t: 0
                };
                for event in events {
                    if self.channels == 1 {
                        output_event = event.into();
                        self.bincode.serialize_into(&mut *stream, &output_event).unwrap();
                        // bincode::serialize_into(&mut *stream, &output_event, my_options).unwrap();
                    } else {
                        self.bincode.serialize_into(&mut *stream, event).unwrap();
                    }

                //     // NOTE: for speed, the following checks only run in debug builds. It's entirely
                //     // possibly to encode non-sensical events if you want to.
                //     debug_assert!(event.coord.x < self.width);
                //     debug_assert!(event.coord.y < self.height);
                //     match event.coord.c {
                //         None => {
                //             debug_assert_eq!(self.channels, 1);
                //         }
                //         Some(c) => {
                //             debug_assert!(c > 0);
                //             debug_assert!(c <= self.channels);
                //             if c == 1 {
                //                 debug_assert!(self.channels > 1);
                //             }
                //         }
                //     }
                //     debug_assert!(event.delta_t <= self.delta_t_max);
                //     out_buf.put(Bytes::from(event));
                }
                // stream.write_all(&out_buf).expect("Unable to write events");
            }
        }
    }

    fn decode_event(&mut self) -> Result<Event, std::io::Error> {
        // let mut buf = vec![0u8; self.event_size as usize];
        match &mut self.input_stream {
            None => {
                panic!("No input stream set")
            }
            Some(stream) => {
                if self.channels == 1 {
                    let event: EventSingle = self.bincode.deserialize_from(stream).unwrap();
                    // bincode::serialize_into(&mut *stream, &output_event, my_options).unwrap();
                    Ok(event.into())
                } else {
                    let event: Event = self.bincode.deserialize_from(stream).unwrap();
                    // self.bincode.serialize_into(&mut *stream, event).unwrap();
                    Ok(event)
                }



                // match stream.read_exact(&mut buf) {
                //     Ok(_) => {
                //
                //         let mut byte_buffer = &buf[..];
                //         let event = Event {
                //             coord: Coord {
                //                 x: byte_buffer.get_u16(),
                //                 y: byte_buffer.get_u16(),
                //                 c: match self.channels {
                //                     1 => { None },
                //                     _ => { Some(byte_buffer.get_u8()) }
                //                 }
                //             },
                //             d: byte_buffer.get_u8(),
                //             delta_t: byte_buffer.get_u32()
                //         };
                //
                //         Ok(event)
                //     }
                //     Err(e) => Err(e),
                // }
            }
        }

    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use rand::Rng;
    use crate::{Codec, Coord, Event};
    use crate::raw::raw_stream::RawStream;

    #[test]
    fn ttt() {
        let n: u32 = rand::thread_rng().gen();
        let mut stream: RawStream = Codec::new();
        stream.open_writer("./TEST_".to_owned() + n.to_string().as_str() + ".addr").expect("Couldn't open file");
        stream.encode_header(50, 100, 53000, 4000, 50000, 1);
        let event: Event = Event {
            coord: Coord {
                x: 10,
                y: 30,
                c: None
            },
            d: 5,
            delta_t: 1000
        };
        stream.encode_event(&event);
        stream.flush_writer();
        stream.open_reader("./TEST_".to_owned() + n.to_string().as_str() + ".addr").expect("Couldn't open file");
        stream.decode_header();
        let res = stream.decode_event();
        match res {
            Ok(decoded_event) => {
                assert_eq!(event, decoded_event);
            }
            Err(_) => {
                panic!("Couldn't decode event")
            }
        }
        stream.encode_header(20, 30, 473289, 477893, 4732987, 3);
        assert!(stream.input_stream.is_none());


        stream.close_writer();
        fs::remove_file("./TEST_".to_owned() + n.to_string().as_str() + ".addr").unwrap();  // Don't check the error
    }
}
