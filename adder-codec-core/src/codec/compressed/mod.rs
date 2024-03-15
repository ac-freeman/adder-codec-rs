pub mod fenwick;
mod source_model;
/// Compressed codec
pub mod stream;

pub const BLOCK_SIZE_BIG: usize = 64;

pub const BLOCK_SIZE_BIG_AREA: usize = BLOCK_SIZE_BIG * BLOCK_SIZE_BIG;

pub type DResidual = i16;
pub const DRESIDUAL_NO_EVENT: DResidual = 256;
pub const DRESIDUAL_SKIP_CUBE: DResidual = 257;
pub type TResidual = i16;

#[cfg(test)]
mod tests {
    use crate::codec::encoder::Encoder;
    use crate::codec::{CodecMetadata, EncoderOptions};
    use crate::{Coord, Event, PlaneSize};

    #[test]
    fn test_create_compressed_stream() {
        // Example of creating a compressed stream and ingesting some events
        let meta = CodecMetadata {
            delta_t_max: 100,
            ref_interval: 100,
            ..Default::default()
        };

        let output = crate::codec::compressed::stream::CompressedOutput::new(meta, Vec::new());
        let mut encoder = Encoder::new_compressed(
            output,
            EncoderOptions::default(PlaneSize {
                width: 100,
                height: 100,
                channels: 1,
            }),
        );
        let meta = *encoder.meta();
        let mut test_event = Event {
            coord: Coord {
                x: 0,
                y: 0,
                c: None,
            },
            d: 5,
            t: 100,
        };
        encoder.ingest_event(test_event).unwrap();
        encoder.flush_writer().unwrap();
        let writer = encoder.close_writer().unwrap().unwrap();

        let tmp_len = writer.len();

        // We should have compressed the partial ADU, and thus have more than the header size
        assert!(writer.len() > meta.header_size);

        let output = crate::codec::compressed::stream::CompressedOutput::new(meta, Vec::new());
        let mut encoder = Encoder::new_compressed(
            output,
            EncoderOptions::default(PlaneSize {
                width: 100,
                height: 100,
                channels: 1,
            }),
        );
        let meta = *encoder.meta();
        dbg!(meta);
        encoder.ingest_event(test_event).unwrap();
        test_event.t += 100;
        encoder.ingest_event(test_event).unwrap();
        test_event.t += 100;
        encoder.ingest_event(test_event).unwrap();
        encoder.flush_writer().unwrap();
        let writer = encoder.close_writer().unwrap().unwrap();

        // Now we've exceeded the DeltaT_max, so we should have written out a frame
        assert!(writer.len() > tmp_len);
    }
}
