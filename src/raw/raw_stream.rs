use std::fs::File;
use std::io::{BufWriter, Write};
use bytes::Bytes;
use crate::{Codec, DeltaT, Event, EventStreamHeader};
use crate::header::MAGIC_RAW;

pub struct RawStream {
    output_stream: Option<BufWriter<File>>,
    input_stream: Option<BufWriter<File>>,
    width: u16,
    height: u16,
    tps: u32,
    ref_interval: u32,
    delta_t_max: u32,
    channels: u8,
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

}