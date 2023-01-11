use crate::raw::stream::Raw;
use crate::{Codec, DeltaT, Event, SourceCamera, TimeMode};
use ndarray::Array3;
use std::error::Error;

pub fn absolute_event_to_dt_event(mut event: Event, last_t: DeltaT) -> Event {
    event.delta_t -= last_t;
    event
}

pub fn migrate_v2(mut input_stream: Raw, mut output_stream: Raw) -> Result<Raw, Box<dyn Error>> {
    let mut data = Vec::new();
    for _ in 0..input_stream.plane.volume() {
        let t = 0_u32;
        data.push(t);
    }
    let mut t_tree: Array3<u32> = Array3::from_shape_vec(
        (
            input_stream.plane.h_usize(),
            input_stream.plane.w_usize(),
            input_stream.plane.c_usize(),
        ),
        data,
    )?;

    loop {
        let mut event = match input_stream.decode_event() {
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

        if output_stream.time_mode == TimeMode::AbsoluteT {
            event.delta_t = *t;

            // If framed video source, we can take advantage of scheme that reduces event rate by half
            if input_stream.codec_version > 0
                && match input_stream.source_camera {
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
                && *t % u32::from(input_stream.ref_interval) > 0
            {
                *t = ((*t / u32::from(input_stream.ref_interval)) + 1)
                    * u32::from(input_stream.ref_interval);
            }
        }

        output_stream.encode_event(&event)?;
    }
    Ok(output_stream)
}

#[cfg(test)]
mod tests {
    use crate::SourceCamera::FramedU8;
    use crate::{Codec, Coord, Event, PlaneSize};
    use rand::Rng;
    use std::{fs, mem};

    /// Test the `migrate_v2` function by making a v1 stream, converting it to v2, and checking the
    /// events
    #[test]
    fn test_migrate_v2() -> Result<(), Box<dyn std::error::Error>> {
        use crate::raw::stream::Raw;
        use crate::utils::stream_migration::migrate_v2;
        use crate::{Codec, DeltaT, SourceCamera, TimeMode};
        use std::{error, io};

        let n: u32 = rand::thread_rng().gen();
        let mut stream: Raw = Codec::new();
        stream
            .open_writer("./TEST_".to_owned() + n.to_string().as_str() + ".adder")
            .expect("Couldn't open file");
        let plane = PlaneSize::new(1, 1, 1).unwrap();
        stream
            .encode_header(
                plane,
                255 * 30,
                255,
                2550,
                1,
                Some(FramedU8),
                Some(TimeMode::DeltaT),
            )
            .unwrap();

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
        stream.encode_event(&event)?;
        stream.encode_event(&event)?;
        stream.encode_event(&event)?;
        let event: Event = Event {
            coord: Coord {
                x: 0,
                y: 0,
                c: None,
            },
            d: 5,
            delta_t: 123,
        };
        stream.encode_event(&event)?;

        stream.close_writer()?;

        stream
            .open_reader("./TEST_".to_owned() + n.to_string().as_str() + ".adder")
            .expect("Couldn't open file");
        stream.decode_header()?;

        let mut output_stream = Raw::new();
        output_stream.open_writer("./TEST_".to_owned() + n.to_string().as_str() + "_v2.adder")?;
        output_stream.encode_header(
            stream.plane.clone(),
            stream.tps,
            stream.ref_interval.clone(),
            stream.delta_t_max,
            2,
            Some(stream.source_camera),
            Some(TimeMode::AbsoluteT),
        )?;

        output_stream = migrate_v2(stream, output_stream)?;
        output_stream.close_writer()?;

        fs::remove_file("./TEST_".to_owned() + n.to_string().as_str() + ".adder").unwrap();

        let mut input_stream = Raw::new();
        input_stream.open_reader("./TEST_".to_owned() + n.to_string().as_str() + "_v2.adder")?;
        input_stream.decode_header()?;

        /*
        Now, the events when converted to v2 should have these t values:
            600, 1365, 2130, 2418
         */
        let mut event = input_stream.decode_event()?;
        assert_eq!(event.coord.x as i32, 0);
        assert_eq!(event.coord.y as i32, 0);
        assert_eq!(event.coord.c, None);
        assert_eq!(event.delta_t as u32, 600);
        assert_eq!(event.d, 5);

        event = input_stream.decode_event()?;
        assert_eq!(event.delta_t as u32, 1365);
        event = input_stream.decode_event()?;
        assert_eq!(event.delta_t as u32, 2130);
        event = input_stream.decode_event()?;
        assert_eq!(event.delta_t as u32, 2418);

        input_stream.close_writer().unwrap();
        fs::remove_file("./TEST_".to_owned() + n.to_string().as_str() + "_v2.adder").unwrap();

        Ok(())
    }

    /// Test the `migrate_v2` function by making a v1 stream, converting it to v2, and checking the
    /// events
    #[test]
    fn test_migrate_v2_nyc() -> Result<(), Box<dyn std::error::Error>> {
        use crate::raw::stream::Error::Eof;
        use crate::raw::stream::Raw;
        use crate::transcoder::source::davis::TranscoderMode::{Framed, RawDavis, RawDvs};
        use crate::utils::stream_migration::migrate_v2;
        use crate::SourceCamera::DavisU8;
        use crate::{Codec, DeltaT, SourceCamera, TimeMode};
        use ndarray::Array3;
        use std::io::Write;
        use std::path::Path;
        use std::time::Instant;
        use std::{error, io};

        let n: u32 = rand::thread_rng().gen();
        let mut stream: Raw = Codec::new();
        stream
            .open_reader("./tests/samples/nyc_v1_1px.adder".to_owned())
            .expect("Couldn't open file");
        stream.decode_header()?;

        let mut output_stream = Raw::new();
        output_stream.open_writer("./TEST_".to_owned() + n.to_string().as_str() + "_v2.adder")?;
        output_stream.encode_header(
            stream.plane.clone(),
            stream.tps,
            stream.ref_interval.clone(),
            stream.delta_t_max,
            2,
            Some(stream.source_camera),
            Some(TimeMode::AbsoluteT),
        )?;

        output_stream = migrate_v2(stream, output_stream)?;
        output_stream.close_writer()?;

        let mut input_stream_gt = Raw::new();
        input_stream_gt.open_reader("./tests/samples/nyc_source_v2_2_1px.adder".to_owned())?;
        input_stream_gt.decode_header()?;

        let mut input_stream_migrate = Raw::new();
        input_stream_migrate
            .open_reader("./TEST_".to_owned() + n.to_string().as_str() + "_v2.adder")?;
        input_stream_migrate.decode_header()?;

        let tmp = mem::size_of::<Event>();

        let mut event_count = 0;
        loop {
            let event_migrate = match input_stream_migrate.decode_event() {
                Ok(ev) => ev,
                Err(_) => {
                    break;
                }
            };
            let event_gt = match input_stream_gt.decode_event() {
                Ok(ev) => ev,
                Err(_) => {
                    break;
                }
            };
            event_count += 1;
            assert_eq!(event_migrate.coord.x as i32, event_gt.coord.x as i32);
            assert_eq!(event_migrate.coord.y as i32, event_gt.coord.y as i32);
            assert_eq!(event_migrate.coord.c, event_gt.coord.c);
            assert_eq!(event_migrate.delta_t as u32, event_gt.delta_t as u32);
            assert_eq!(event_migrate.d, event_gt.d);
        }
        assert_eq!(event_count, 5);

        fs::remove_file("./TEST_".to_owned() + n.to_string().as_str() + "_v2.adder").unwrap();

        Ok(())
    }
}
