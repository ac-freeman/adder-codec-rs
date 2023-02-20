use adder_codec_core::codec::decoder::Decoder;
use adder_codec_core::codec::encoder::Encoder;
use adder_codec_core::{DeltaT, Event, SourceCamera, TimeMode};
use bitstream_io::BigEndian;
use ndarray::Array3;
use std::error::Error;
use std::io::{Read, Seek, Write};

pub fn absolute_event_to_dt_event(mut event: Event, last_t: DeltaT) -> Event {
    event.delta_t -= last_t;
    event
}

pub fn migrate_v2<W: Write, R: Read + Seek>(
    mut input_stream: Decoder<R>,
    mut bitreader: &mut bitstream_io::BitReader<R, BigEndian>,
    mut output_stream: Encoder<W>,
) -> Result<Encoder<W>, Box<dyn Error>> {
    let mut t_tree: Array3<u32> = Array3::from_shape_vec(
        (
            input_stream.meta().plane.h_usize(),
            input_stream.meta().plane.w_usize(),
            input_stream.meta().plane.c_usize(),
        ),
        vec![0_u32; input_stream.meta().plane.volume()],
    )?;

    loop {
        let mut event = match input_stream.digest_event(bitreader) {
            Ok(event) => event,
            Err(_) => {
                break;
            }
        };
        let t = &mut t_tree[[
            event.coord.y_usize(),
            event.coord.x_usize(),
            event.coord.c_usize(),
        ]];

        *t += event.delta_t;

        if output_stream.meta().time_mode == TimeMode::AbsoluteT {
            event.delta_t = *t;

            // If framed video source, we can take advantage of scheme that reduces event rate by half
            if input_stream.meta().codec_version > 0
                && match input_stream.meta().source_camera {
                    SourceCamera::FramedU8
                    | SourceCamera::FramedU16
                    | SourceCamera::FramedU32
                    | SourceCamera::FramedU64
                    | SourceCamera::FramedF32
                    | SourceCamera::FramedF64 => true,
                    SourceCamera::Dvs
                    | SourceCamera::DavisU8
                    | SourceCamera::Atis
                    | SourceCamera::Asint => false,
                }
                && *t % input_stream.meta().ref_interval > 0
            {
                *t = ((*t / input_stream.meta().ref_interval) + 1)
                    * input_stream.meta().ref_interval;
            }
        }

        output_stream.ingest_event(&event)?;
    }
    Ok(output_stream)
}

#[cfg(test)]
mod tests {
    use crate::framer::driver::FramerMode::INSTANTANEOUS;
    use crate::framer::driver::{FrameSequence, Framer, FramerBuilder};
    use crate::utils::stream_migration::absolute_event_to_dt_event;
    use adder_codec_core::codec::decoder::Decoder;
    use adder_codec_core::codec::encoder::Encoder;
    use adder_codec_core::codec::raw::stream::{RawInput, RawOutput};
    use adder_codec_core::codec::{CodecMetadata, ReadCompression, WriteCompression};
    use adder_codec_core::SourceCamera::FramedU8;
    use adder_codec_core::TimeMode::AbsoluteT;
    use adder_codec_core::{Coord, Event, PlaneSize, TimeMode};
    use bitstream_io::{BigEndian, BitReader};
    use ndarray::Array3;
    use std::fs::File;
    use std::io::{BufReader, BufWriter, Cursor};

    /// Test the `migrate_v2` function by making a v1 stream, converting it to v2, and checking the
    /// events
    #[test]
    fn test_migrate_v2() -> Result<(), Box<dyn std::error::Error>> {
        use crate::utils::stream_migration::migrate_v2;

        let plane = PlaneSize::new(1, 1, 1).unwrap();

        let output = Vec::new();
        let bufwriter = BufWriter::new(output);
        let compression = <RawOutput<_> as WriteCompression<BufWriter<Vec<u8>>>>::new(
            CodecMetadata {
                codec_version: 1, // Make this a v1 stream
                header_size: 0,
                time_mode: TimeMode::DeltaT,
                plane,
                tps: 255 * 30,
                ref_interval: 255,
                delta_t_max: 2550,
                event_size: 0,
                source_camera: FramedU8,
            },
            bufwriter,
        );
        let mut stream = Encoder::new(Box::new(compression));

        // Encode the events
        let event: Event = Event {
            coord: Coord {
                x: 0,
                y: 0,
                c: None,
            },
            d: 5,
            delta_t: 600,
        };
        stream.ingest_event(&event)?;
        stream.ingest_event(&event)?;
        stream.ingest_event(&event)?;
        let event: Event = Event {
            coord: Coord {
                x: 0,
                y: 0,
                c: None,
            },
            d: 5,
            delta_t: 123,
        };
        stream.ingest_event(&event)?;

        let writer = stream.close_writer().unwrap().unwrap();
        let bytes = writer.into_inner().unwrap();
        let tmp = Cursor::new(&*bytes);
        let bufreader = BufReader::new(tmp);
        let compression = <RawInput as ReadCompression<BufReader<Cursor<&[u8]>>>>::new();
        let mut bitreader = BitReader::endian(bufreader, BigEndian);
        let reader = Decoder::new(Box::new(compression), &mut bitreader).unwrap();

        let output = Vec::new();
        let bufwriter = BufWriter::new(output);
        let compression = <RawOutput<_> as WriteCompression<BufWriter<Vec<u8>>>>::new(
            CodecMetadata {
                codec_version: 2, // Make this a v1 stream
                header_size: 0,
                time_mode: TimeMode::AbsoluteT,
                plane,
                tps: 255 * 30,
                ref_interval: 255,
                delta_t_max: 2550,
                event_size: 0,
                source_camera: FramedU8,
            },
            bufwriter,
        );
        let mut stream = Encoder::new(Box::new(compression));

        stream = migrate_v2(reader, &mut bitreader, stream)?;

        let writer = stream.close_writer().unwrap().unwrap();
        let bytes = writer.into_inner().unwrap();
        let tmp = Cursor::new(&*bytes);
        let bufreader = BufReader::new(tmp);
        let compression = <RawInput as ReadCompression<BufReader<Cursor<&[u8]>>>>::new();
        let mut bitreader = BitReader::endian(bufreader, BigEndian);
        let mut reader = Decoder::new(Box::new(compression), &mut bitreader).unwrap();

        /*
        Now, the events when converted to v2 with absolute_t mode should have these t values:
            600, 1365, 2130, 2418
         */
        let mut event = reader.digest_event(&mut bitreader)?;
        assert_eq!(event.coord.x as i32, 0);
        assert_eq!(event.coord.y as i32, 0);
        assert_eq!(event.coord.c, None);
        let dt = event.delta_t;
        assert_eq!(dt, 600);
        assert_eq!(event.d, 5);

        event = reader.digest_event(&mut bitreader)?;
        let dt = event.delta_t;
        assert_eq!(dt, 1365);
        event = reader.digest_event(&mut bitreader)?;
        let dt = event.delta_t;
        assert_eq!(dt, 2130);
        event = reader.digest_event(&mut bitreader)?;
        let dt = event.delta_t;
        assert_eq!(dt, 2418);

        Ok(())
    }

    /// Test the `migrate_v2` function by making a v1 stream, converting it to v2, and checking the
    /// events
    #[test]
    fn test_migrate_v2_nyc() -> Result<(), Box<dyn std::error::Error>> {
        use crate::utils::stream_migration::migrate_v2;

        let bufreader = BufReader::new(File::open("./tests/samples/nyc_v1_1px.adder")?);
        let compression = <RawInput as ReadCompression<BufReader<File>>>::new();
        let mut bitreader = BitReader::endian(bufreader, BigEndian);
        let reader = Decoder::new(Box::new(compression), &mut bitreader).unwrap();

        let output = Vec::new();
        let bufwriter = BufWriter::new(output);
        let mut meta = *reader.meta();
        meta.codec_version = 2;
        meta.time_mode = AbsoluteT;
        let compression =
            <RawOutput<_> as WriteCompression<BufWriter<Vec<u8>>>>::new(meta, bufwriter);
        let mut stream = Encoder::new(Box::new(compression));

        stream = migrate_v2(reader, &mut bitreader, stream)?;

        let writer = stream.close_writer().unwrap().unwrap();
        let bytes = writer.into_inner().unwrap();
        let tmp = Cursor::new(&*bytes);
        let bufreader = BufReader::new(tmp);
        let compression = <RawInput as ReadCompression<BufReader<Cursor<&[u8]>>>>::new();
        let mut bitreader_migrate = BitReader::endian(bufreader, BigEndian);
        let mut reader_migrate =
            Decoder::new(Box::new(compression), &mut bitreader_migrate).unwrap();

        let bufreader = BufReader::new(File::open("./tests/samples/nyc_source_v2_2_1px.adder")?);
        let compression = <RawInput as ReadCompression<BufReader<File>>>::new();
        let mut bitreader_gt = BitReader::endian(bufreader, BigEndian);
        let mut reader_gt = Decoder::new(Box::new(compression), &mut bitreader_gt).unwrap();

        let mut event_count = 0;
        loop {
            let event_migrate = match reader_migrate.digest_event(&mut bitreader_migrate) {
                Ok(ev) => ev,
                Err(_) => {
                    break;
                }
            };
            let event_gt = match reader_gt.digest_event(&mut bitreader_gt) {
                Ok(ev) => ev,
                Err(_) => {
                    break;
                }
            };
            event_count += 1;
            assert_eq!(event_migrate.coord.x as i32, event_gt.coord.x as i32);
            assert_eq!(event_migrate.coord.y as i32, event_gt.coord.y as i32);
            assert_eq!(event_migrate.coord.c, event_gt.coord.c);
            let dt = event_migrate.delta_t;
            let dt_g = event_gt.delta_t;
            assert_eq!(dt, dt_g);
            assert_eq!(event_migrate.d, event_gt.d);
        }
        assert_eq!(event_count, 5);

        Ok(())
    }

    /// Test that when reconstructing framed video, we get the same results with both `DeltaT` and
    /// `AbsoluteT` time modes
    #[test]
    fn test_migrate_v2_bunny_1px() -> Result<(), Box<dyn std::error::Error>> {
        let bufreader = BufReader::new(File::open("./tests/samples/bunny_v2_t.adder")?);
        let compression = <RawInput as ReadCompression<BufReader<File>>>::new();
        let mut bitreader_t = BitReader::endian(bufreader, BigEndian);
        let mut input_stream_t = Decoder::new(Box::new(compression), &mut bitreader_t).unwrap();

        let reconstructed_frame_rate = 30.0;

        let mut frame_sequence_t: FrameSequence<u8> =
            FramerBuilder::new(input_stream_t.meta().plane.clone(), 64)
                .codec_version(input_stream_t.meta().codec_version, TimeMode::AbsoluteT)
                .time_parameters(
                    input_stream_t.meta().tps,
                    input_stream_t.meta().ref_interval,
                    input_stream_t.meta().delta_t_max,
                    reconstructed_frame_rate,
                )
                .mode(INSTANTANEOUS)
                .source(
                    input_stream_t.get_source_type(),
                    input_stream_t.meta().source_camera,
                )
                .finish();

        let bufreader = BufReader::new(File::open("./tests/samples/bunny_v2_dt.adder")?);
        let compression = <RawInput as ReadCompression<BufReader<File>>>::new();
        let mut bitreader_dt = BitReader::endian(bufreader, BigEndian);
        let mut input_stream_dt = Decoder::new(Box::new(compression), &mut bitreader_dt).unwrap();

        let mut frame_sequence_dt: FrameSequence<u8> =
            FramerBuilder::new(input_stream_dt.meta().plane.clone(), 64)
                .codec_version(input_stream_dt.meta().codec_version, TimeMode::DeltaT)
                .time_parameters(
                    input_stream_dt.meta().tps,
                    input_stream_dt.meta().ref_interval,
                    input_stream_dt.meta().delta_t_max,
                    reconstructed_frame_rate,
                )
                .mode(INSTANTANEOUS)
                .source(
                    input_stream_dt.get_source_type(),
                    input_stream_dt.meta().source_camera,
                )
                .finish();

        let mut event_count = 0;
        let mut last_t = 0;
        let mut t_frame: Option<Vec<Array3<Option<u8>>>> = None;
        let mut dt_frame;
        loop {
            let event_t = match input_stream_t.digest_event(&mut bitreader_t) {
                Ok(ev) => ev,
                Err(_) => {
                    break;
                }
            };
            if frame_sequence_t.ingest_event(&mut event_t.clone()) {
                t_frame = frame_sequence_t.pop_next_frame();
            }

            let event_dt = match input_stream_dt.digest_event(&mut bitreader_dt) {
                Ok(ev) => ev,
                Err(_) => {
                    break;
                }
            };
            if frame_sequence_dt.ingest_event(&mut event_dt.clone()) {
                dt_frame = frame_sequence_dt.pop_next_frame();

                let dt_val = dt_frame.unwrap()[0][[0, 0, 0]].unwrap();
                let t_val = t_frame.clone().unwrap()[0][[0, 0, 0]].unwrap();
                assert_eq!(dt_val, t_val);
            }

            event_count += 1;

            let event_t_dt = absolute_event_to_dt_event(event_t, last_t);
            last_t = event_t.delta_t;

            // We already know it's a framed source
            last_t = ((last_t / input_stream_dt.meta().ref_interval) + 1)
                * input_stream_dt.meta().ref_interval;

            assert_eq!(event_t_dt.coord.x as i32, event_dt.coord.x as i32);
            assert_eq!(event_t_dt.coord.y as i32, event_dt.coord.y as i32);
            assert_eq!(event_t_dt.coord.c, event_dt.coord.c);
            let dt_mig = event_t_dt.delta_t;
            let dt_gt = event_dt.delta_t;
            assert_eq!(dt_mig, dt_gt);
            assert_eq!(event_t_dt.d, event_dt.d);
        }
        assert_eq!(event_count, 333);

        Ok(())
    }

    #[test]
    fn test_migrate_v2_bunny_8() -> Result<(), Box<dyn std::error::Error>> {
        let bufreader = BufReader::new(File::open("./tests/samples/bunny_v2_t_3.adder")?);
        let compression = <RawInput as ReadCompression<BufReader<File>>>::new();
        let mut bitreader_t = BitReader::endian(bufreader, BigEndian);
        let mut input_stream_t = Decoder::new(Box::new(compression), &mut bitreader_t).unwrap();

        let reconstructed_frame_rate = 30.0;

        let mut frame_sequence_t: FrameSequence<u8> =
            FramerBuilder::new(input_stream_t.meta().plane.clone(), 500)
                .codec_version(input_stream_t.meta().codec_version, TimeMode::AbsoluteT)
                .time_parameters(
                    input_stream_t.meta().tps,
                    input_stream_t.meta().ref_interval,
                    input_stream_t.meta().delta_t_max,
                    reconstructed_frame_rate,
                )
                .mode(INSTANTANEOUS)
                .source(
                    input_stream_t.get_source_type(),
                    input_stream_t.meta().source_camera,
                )
                .finish();

        let bufreader = BufReader::new(File::open("./tests/samples/bunny_v2_dt_3.adder")?);
        let compression = <RawInput as ReadCompression<BufReader<File>>>::new();
        let mut bitreader_dt = BitReader::endian(bufreader, BigEndian);
        let mut input_stream_dt = Decoder::new(Box::new(compression), &mut bitreader_dt).unwrap();

        let mut frame_sequence_dt: FrameSequence<u8> =
            FramerBuilder::new(input_stream_dt.meta().plane.clone(), 500)
                .codec_version(input_stream_dt.meta().codec_version, TimeMode::DeltaT)
                .time_parameters(
                    input_stream_dt.meta().tps,
                    input_stream_dt.meta().ref_interval,
                    input_stream_dt.meta().delta_t_max,
                    reconstructed_frame_rate,
                )
                .mode(INSTANTANEOUS)
                .source(
                    input_stream_dt.get_source_type(),
                    input_stream_dt.meta().source_camera,
                )
                .finish();

        let mut event_count = 0;
        let mut t_tree: Array3<u32> = Array3::from_shape_vec(
            (
                input_stream_dt.meta().plane.h_usize(),
                input_stream_dt.meta().plane.w_usize(),
                input_stream_dt.meta().plane.c_usize(),
            ),
            vec![0_u32; input_stream_dt.meta().plane.volume()],
        )?;
        let mut t_frame: Option<Vec<Array3<Option<u8>>>> = None;
        let mut dt_frame;
        loop {
            let event_t = match input_stream_t.digest_event(&mut bitreader_t) {
                Ok(ev) => ev,
                Err(_) => {
                    break;
                }
            };
            if event_t.coord.y == 15
                && event_t.coord.x == 123
                && event_t.coord.c_usize() == 0
                && event_count > 540
            {
                dbg!(event_t);
            }

            let a_t = frame_sequence_t.ingest_event(&mut event_t.clone());

            if a_t {
                t_frame = frame_sequence_t.pop_next_frame();
            }

            let event_dt = match input_stream_dt.digest_event(&mut bitreader_dt) {
                Ok(ev) => ev,
                Err(_) => {
                    break;
                }
            };
            let a_dt = frame_sequence_dt.ingest_event(&mut event_dt.clone());

            if a_dt {
                dt_frame = frame_sequence_dt.pop_next_frame();

                for c in 0..input_stream_dt.meta().plane.c_usize() {
                    for y in 0..input_stream_dt.meta().plane.h_usize() {
                        for x in 0..input_stream_dt.meta().plane.w_usize() {
                            let dt_val =
                                dt_frame.clone().unwrap().last().unwrap()[[y, x, c]].unwrap();
                            let t_val =
                                t_frame.clone().unwrap().last().unwrap()[[y, x, c]].unwrap();
                            assert_eq!(dt_val, t_val);
                        }
                    }
                }
                // assert_eq!(dt_frame.unwrap()[0], t_frame.clone().unwrap()[0]);

                // let dt_val = dt_frame.unwrap()[0][[0, 0, 0]].unwrap();
                // let t_val = t_frame.clone().unwrap()[0][[0, 0, 0]].unwrap();
                // assert_eq!(dt_val, t_val);
            }

            event_count += 1;
            let last_t = &mut t_tree[[
                event_t.coord.y_usize(),
                event_t.coord.x_usize(),
                event_t.coord.c_usize(),
            ]];

            let event_t_dt = absolute_event_to_dt_event(event_t, *last_t);
            *last_t = event_t.delta_t;

            // We already know it's a framed source
            if *last_t % input_stream_dt.meta().ref_interval != 0 {
                *last_t = ((*last_t / input_stream_dt.meta().ref_interval) + 1)
                    * input_stream_dt.meta().ref_interval;
            }

            assert_eq!(event_t_dt.coord.x as i32, event_dt.coord.x as i32);
            assert_eq!(event_t_dt.coord.y as i32, event_dt.coord.y as i32);
            assert_eq!(event_t_dt.coord.c, event_dt.coord.c);
            let dt_mig = event_t_dt.delta_t;
            let dt_gt = event_dt.delta_t;
            assert_eq!(dt_mig, dt_gt);
            assert_eq!(event_t_dt.d, event_dt.d);
        }
        assert_eq!(event_count, 675693);

        Ok(())
    }
}
