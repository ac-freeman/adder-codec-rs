pub mod adu;
pub mod blocks;
/// Compressed codec
pub mod stream;

#[cfg(test)]
mod tests {
    use crate::codec::compressed::stream;
    use crate::codec::encoder::Encoder;
    use crate::codec::{CodecMetadata, EncoderOptions};
    use crate::{Coord, Event};

    #[test]
    fn test_create_compressed_stream() {
        // Example of creating a compressed stream and ingesting some events
        let meta = CodecMetadata {
            delta_t_max: 100,
            ref_interval: 100,
            ..Default::default()
        };

        let output = crate::codec::compressed::stream::CompressedOutput::new(meta, Vec::new());
        let mut encoder = Encoder::new_compressed(output, EncoderOptions::default());
        let meta = encoder.meta().clone();
        let mut test_event = Event {
            coord: Coord {
                x: 0,
                y: 0,
                c: None,
            },
            d: 5,
            delta_t: 100,
        };
        encoder.ingest_event(test_event);
        encoder.flush_writer().unwrap();
        let writer = encoder.close_writer().unwrap().unwrap();

        dbg!(writer.len());
        // It should still be just the header, because we haven't integrated enough events
        // to write out a frame (haven't reached DeltaT_max)
        assert!(writer.len() == meta.header_size);
        dbg!(writer);

        let output = crate::codec::compressed::stream::CompressedOutput::new(meta, Vec::new());
        let mut encoder = Encoder::new_compressed(output, EncoderOptions::default());
        let meta = encoder.meta().clone();
        encoder.ingest_event(test_event);
        test_event.delta_t += 100;
        encoder.ingest_event(test_event);
        test_event.delta_t += 100;
        encoder.ingest_event(test_event);
        encoder.flush_writer().unwrap();
        let writer = encoder.close_writer().unwrap().unwrap();

        dbg!(writer.len());
        assert!(writer.len() > meta.header_size);
        dbg!(writer);
    }
}
