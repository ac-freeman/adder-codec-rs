use std::fs::File;
use std::io::{BufWriter, Write};
use bytes::Bytes;
use crate::{Codec, Event, EventStreamHeader};
use crate::header::MAGIC_RAW;

pub struct RawStream {
    output_stream: Option<BufWriter<File>>,
    input_stream: Option<BufWriter<File>>,
}

impl Codec for RawStream {
    fn new() -> Self {
        RawStream {
            output_stream: None,
            input_stream: None
        }
    }

    fn set_output_stream(&mut self, stream: Option<BufWriter<File>>) {
        self.output_stream = stream;
    }

    fn set_input_stream(&mut self, stream: Option<BufWriter<File>>) {
        self.input_stream = stream;
    }

    fn serialize_header(&mut self,
                        width: u16,
                        height: u16,
                        tps: u32,
                        ref_interval: u32,
                        delta_t_max: u32,
                        channels: u8) {
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
    }

    fn encode_event(&mut self, event: &Event) {
        match &mut self.output_stream {
            None => {}
            Some(stream) => {
                stream.write_all(&Bytes::from(event).to_vec())
                    .expect("Unable to write event");
            }
        }
    }

}