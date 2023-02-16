// use crate::codec::raw::stream::Raw;
// use crate::codec::{Decoder, Encoder};
// use crate::{DeltaT, Event, SourceCamera, TimeMode};
// use ndarray::Array3;
// use std::error::Error;
// use std::io::{Seek, Write};
//
// pub fn absolute_event_to_dt_event(mut event: Event, last_t: DeltaT) -> Event {
//     event.delta_t -= last_t;
//     event
// }
//
// pub fn migrate_v2<W: Write + Seek>(
//     mut input_stream: Raw<W>,
//     mut output_stream: Raw<W>,
// ) -> Result<Raw<W>, Box<dyn Error>> {
//     let mut t_tree: Array3<u32> = Array3::from_shape_vec(
//         (
//             input_stream.plane.h_usize(),
//             input_stream.plane.w_usize(),
//             input_stream.plane.c_usize(),
//         ),
//         vec![0_u32; input_stream.plane.volume()],
//     )?;
//
//     loop {
//         let mut event = match input_stream.decode_event() {
//             Ok(event) => event,
//             Err(_) => {
//                 break;
//             }
//         };
//         let t = &mut t_tree[[
//             event.coord.y_usize(),
//             event.coord.x_usize(),
//             event.coord.c_usize(),
//         ]];
//
//         *t += event.delta_t;
//
//         if output_stream.time_mode == TimeMode::AbsoluteT {
//             event.delta_t = *t;
//
//             // If framed video source, we can take advantage of scheme that reduces event rate by half
//             if input_stream.codec_version > 0
//                 && match input_stream.source_camera {
//                     SourceCamera::FramedU8
//                     | SourceCamera::FramedU16
//                     | SourceCamera::FramedU32
//                     | SourceCamera::FramedU64
//                     | SourceCamera::FramedF32
//                     | SourceCamera::FramedF64 => true,
//                     SourceCamera::Dvs
//                     | SourceCamera::DavisU8
//                     | SourceCamera::Atis
//                     | SourceCamera::Asint => false,
//                 }
//                 && *t % input_stream.ref_interval > 0
//             {
//                 *t = ((*t / input_stream.ref_interval) + 1) * input_stream.ref_interval;
//             }
//         }
//
//         output_stream.encode_event(&event)?;
//     }
//     Ok(output_stream)
// }
//
// #[cfg(test)]
// mod tests {
//     use crate::codec::raw::stream::Raw;
//     use crate::codec::Codec;
//     use crate::framer::driver::FramerMode::INSTANTANEOUS;
//     use crate::framer::driver::{FrameSequence, Framer, FramerBuilder};
//     use crate::utils::stream_migration::absolute_event_to_dt_event;
//     use crate::SourceCamera::FramedU8;
//     use crate::{Coord, Event, PlaneSize, TimeMode};
//     use ndarray::Array3;
//     use rand::Rng;
//     use std::{fs, mem};
//
//     /// Test the `migrate_v2` function by making a v1 stream, converting it to v2, and checking the
//     /// events
//     #[test]
//     fn test_migrate_v2() -> Result<(), Box<dyn std::error::Error>> {
//         use crate::utils::stream_migration::migrate_v2;
//         use crate::TimeMode;
//
//         let n: u32 = rand::thread_rng().gen();
//         let mut stream: Raw = Codec::new();
//         stream
//             .open_writer("./TEST_".to_owned() + n.to_string().as_str() + ".adder")
//             .expect("Couldn't open file");
//         let plane = PlaneSize::new(1, 1, 1).unwrap();
//         stream
//             .encode_header(
//                 plane,
//                 255 * 30,
//                 255,
//                 2550,
//                 1,
//                 Some(FramedU8),
//                 Some(TimeMode::DeltaT),
//             )
//             .unwrap();
//
//         // Encode the events
//         let event: Event = Event {
//             coord: Coord {
//                 x: 0,
//                 y: 0,
//                 c: None,
//             },
//             d: 5,
//             delta_t: 600,
//         };
//         stream.encode_event(&event)?;
//         stream.encode_event(&event)?;
//         stream.encode_event(&event)?;
//         let event: Event = Event {
//             coord: Coord {
//                 x: 0,
//                 y: 0,
//                 c: None,
//             },
//             d: 5,
//             delta_t: 123,
//         };
//         stream.encode_event(&event)?;
//
//         stream.close_writer()?;
//
//         stream
//             .open_reader("./TEST_".to_owned() + n.to_string().as_str() + ".adder")
//             .expect("Couldn't open file");
//         stream.decode_header()?;
//
//         let mut output_stream = Raw::new();
//         output_stream.open_writer("./TEST_".to_owned() + n.to_string().as_str() + "_v2.adder")?;
//         output_stream.encode_header(
//             stream.plane.clone(),
//             stream.tps,
//             stream.ref_interval,
//             stream.delta_t_max,
//             2,
//             Some(stream.source_camera),
//             Some(TimeMode::AbsoluteT),
//         )?;
//
//         output_stream = migrate_v2(stream, output_stream)?;
//         output_stream.close_writer()?;
//
//         fs::remove_file("./TEST_".to_owned() + n.to_string().as_str() + ".adder").unwrap();
//
//         let mut input_stream = Raw::new();
//         input_stream.open_reader("./TEST_".to_owned() + n.to_string().as_str() + "_v2.adder")?;
//         input_stream.decode_header()?;
//
//         /*
//         Now, the events when converted to v2 should have these t values:
//             600, 1365, 2130, 2418
//          */
//         let mut event = input_stream.decode_event()?;
//         assert_eq!(event.coord.x as i32, 0);
//         assert_eq!(event.coord.y as i32, 0);
//         assert_eq!(event.coord.c, None);
//         let dt = event.delta_t;
//         assert_eq!(dt, 600);
//         assert_eq!(event.d, 5);
//
//         event = input_stream.decode_event()?;
//         let dt = event.delta_t;
//         assert_eq!(dt, 1365);
//         event = input_stream.decode_event()?;
//         let dt = event.delta_t;
//         assert_eq!(dt, 2130);
//         event = input_stream.decode_event()?;
//         let dt = event.delta_t;
//         assert_eq!(dt, 2418);
//
//         input_stream.close_writer().unwrap();
//         fs::remove_file("./TEST_".to_owned() + n.to_string().as_str() + "_v2.adder").unwrap();
//
//         Ok(())
//     }
//
//     /// Test the `migrate_v2` function by making a v1 stream, converting it to v2, and checking the
//     /// events
//     #[test]
//     fn test_migrate_v2_nyc() -> Result<(), Box<dyn std::error::Error>> {
//         use crate::codec::raw::stream::Raw;
//
//         use crate::utils::stream_migration::migrate_v2;
//
//         use crate::TimeMode;
//
//         let n: u32 = rand::thread_rng().gen();
//         let mut stream: Raw = Codec::new();
//         stream
//             .open_reader("./tests/samples/nyc_v1_1px.adder")
//             .expect("Couldn't open file");
//         stream.decode_header()?;
//
//         let mut output_stream = Raw::new();
//         output_stream.open_writer("./TEST_".to_owned() + n.to_string().as_str() + "_v2.adder")?;
//         output_stream.encode_header(
//             stream.plane.clone(),
//             stream.tps,
//             stream.ref_interval,
//             stream.delta_t_max,
//             2,
//             Some(stream.source_camera),
//             Some(TimeMode::AbsoluteT),
//         )?;
//
//         output_stream = migrate_v2(stream, output_stream)?;
//         output_stream.close_writer()?;
//
//         let mut input_stream_gt = Raw::new();
//         input_stream_gt.open_reader("./tests/samples/nyc_source_v2_2_1px.adder")?;
//         input_stream_gt.decode_header()?;
//
//         let mut input_stream_migrate = Raw::new();
//         input_stream_migrate
//             .open_reader("./TEST_".to_owned() + n.to_string().as_str() + "_v2.adder")?;
//         input_stream_migrate.decode_header()?;
//
//         let _tmp = mem::size_of::<Event>();
//
//         let mut event_count = 0;
//         loop {
//             let event_migrate = match input_stream_migrate.decode_event() {
//                 Ok(ev) => ev,
//                 Err(_) => {
//                     break;
//                 }
//             };
//             let event_gt = match input_stream_gt.decode_event() {
//                 Ok(ev) => ev,
//                 Err(_) => {
//                     break;
//                 }
//             };
//             event_count += 1;
//             assert_eq!(event_migrate.coord.x as i32, event_gt.coord.x as i32);
//             assert_eq!(event_migrate.coord.y as i32, event_gt.coord.y as i32);
//             assert_eq!(event_migrate.coord.c, event_gt.coord.c);
//             let dt = event_migrate.delta_t;
//             let dt_g = event_gt.delta_t;
//             assert_eq!(dt, dt_g);
//             assert_eq!(event_migrate.d, event_gt.d);
//         }
//         assert_eq!(event_count, 5);
//
//         fs::remove_file("./TEST_".to_owned() + n.to_string().as_str() + "_v2.adder").unwrap();
//
//         Ok(())
//     }
//
//     #[test]
//     fn test_migrate_v2_bunny_1px() -> Result<(), Box<dyn std::error::Error>> {
//         use crate::codec::raw::stream::Raw;
//
//         let mut input_stream_t = Raw::new();
//         input_stream_t.open_reader("./tests/samples/bunny_v2_t.adder")?;
//         input_stream_t.decode_header()?;
//
//         let reconstructed_frame_rate = 30.0;
//
//         let mut frame_sequence_t: FrameSequence<u8> =
//             FramerBuilder::new(input_stream_t.plane.clone(), 64)
//                 .codec_version(input_stream_t.codec_version, TimeMode::AbsoluteT)
//                 .time_parameters(
//                     input_stream_t.tps,
//                     input_stream_t.ref_interval,
//                     input_stream_t.delta_t_max,
//                     reconstructed_frame_rate,
//                 )
//                 .mode(INSTANTANEOUS)
//                 .source(
//                     input_stream_t.get_source_type(),
//                     input_stream_t.source_camera,
//                 )
//                 .finish();
//
//         let mut input_stream_dt = Raw::new();
//         input_stream_dt.open_reader("./tests/samples/bunny_v2_dt.adder")?;
//         input_stream_dt.decode_header()?;
//
//         let mut frame_sequence_dt: FrameSequence<u8> =
//             FramerBuilder::new(input_stream_dt.plane.clone(), 64)
//                 .codec_version(input_stream_dt.codec_version, TimeMode::DeltaT)
//                 .time_parameters(
//                     input_stream_dt.tps,
//                     input_stream_dt.ref_interval,
//                     input_stream_dt.delta_t_max,
//                     reconstructed_frame_rate,
//                 )
//                 .mode(INSTANTANEOUS)
//                 .source(
//                     input_stream_dt.get_source_type(),
//                     input_stream_dt.source_camera,
//                 )
//                 .finish();
//
//         let mut event_count = 0;
//         let mut last_t = 0;
//         let mut t_frame: Option<Vec<Array3<Option<u8>>>> = None;
//         let mut dt_frame;
//         loop {
//             let event_t = match input_stream_t.decode_event() {
//                 Ok(ev) => ev,
//                 Err(_) => {
//                     break;
//                 }
//             };
//             if frame_sequence_t.ingest_event(&mut event_t.clone()) {
//                 t_frame = frame_sequence_t.pop_next_frame();
//             }
//
//             let event_dt = match input_stream_dt.decode_event() {
//                 Ok(ev) => ev,
//                 Err(_) => {
//                     break;
//                 }
//             };
//             if frame_sequence_dt.ingest_event(&mut event_dt.clone()) {
//                 dt_frame = frame_sequence_dt.pop_next_frame();
//
//                 let dt_val = dt_frame.unwrap()[0][[0, 0, 0]].unwrap();
//                 let t_val = t_frame.clone().unwrap()[0][[0, 0, 0]].unwrap();
//                 assert_eq!(dt_val, t_val);
//             }
//
//             event_count += 1;
//
//             let event_t_dt = absolute_event_to_dt_event(event_t, last_t);
//             last_t = event_t.delta_t;
//
//             // We already know it's a framed source
//             last_t = ((last_t / input_stream_dt.ref_interval) + 1) * input_stream_dt.ref_interval;
//
//             assert_eq!(event_t_dt.coord.x as i32, event_dt.coord.x as i32);
//             assert_eq!(event_t_dt.coord.y as i32, event_dt.coord.y as i32);
//             assert_eq!(event_t_dt.coord.c, event_dt.coord.c);
//             let dt_mig = event_t_dt.delta_t;
//             let dt_gt = event_dt.delta_t;
//             assert_eq!(dt_mig, dt_gt);
//             assert_eq!(event_t_dt.d, event_dt.d);
//         }
//         assert_eq!(event_count, 333);
//
//         Ok(())
//     }
//
//     #[test]
//     fn test_migrate_v2_bunny_8() -> Result<(), Box<dyn std::error::Error>> {
//         use crate::codec::raw::stream::Raw;
//
//         let mut input_stream_t = Raw::new();
//         input_stream_t.open_reader("./tests/samples/bunny_v2_t_3.adder")?;
//         input_stream_t.decode_header()?;
//
//         let reconstructed_frame_rate = 30.0;
//
//         let mut frame_sequence_t: FrameSequence<u8> =
//             FramerBuilder::new(input_stream_t.plane.clone(), 500)
//                 .codec_version(input_stream_t.codec_version, TimeMode::AbsoluteT)
//                 .time_parameters(
//                     input_stream_t.tps,
//                     input_stream_t.ref_interval,
//                     input_stream_t.delta_t_max,
//                     reconstructed_frame_rate,
//                 )
//                 .mode(INSTANTANEOUS)
//                 .source(
//                     input_stream_t.get_source_type(),
//                     input_stream_t.source_camera,
//                 )
//                 .finish();
//
//         let mut input_stream_dt = Raw::new();
//         input_stream_dt.open_reader("./tests/samples/bunny_v2_dt_3.adder")?;
//         input_stream_dt.decode_header()?;
//
//         let mut frame_sequence_dt: FrameSequence<u8> =
//             FramerBuilder::new(input_stream_dt.plane.clone(), 500)
//                 .codec_version(input_stream_dt.codec_version, TimeMode::DeltaT)
//                 .time_parameters(
//                     input_stream_dt.tps,
//                     input_stream_dt.ref_interval,
//                     input_stream_dt.delta_t_max,
//                     reconstructed_frame_rate,
//                 )
//                 .mode(INSTANTANEOUS)
//                 .source(
//                     input_stream_dt.get_source_type(),
//                     input_stream_dt.source_camera,
//                 )
//                 .finish();
//
//         let mut event_count = 0;
//         let mut t_tree: Array3<u32> = Array3::from_shape_vec(
//             (
//                 input_stream_dt.plane.h_usize(),
//                 input_stream_dt.plane.w_usize(),
//                 input_stream_dt.plane.c_usize(),
//             ),
//             vec![0_u32; input_stream_dt.plane.volume()],
//         )?;
//         let mut t_frame: Option<Vec<Array3<Option<u8>>>> = None;
//         let mut dt_frame;
//         loop {
//             let event_t = match input_stream_t.decode_event() {
//                 Ok(ev) => ev,
//                 Err(_) => {
//                     break;
//                 }
//             };
//             if event_t.coord.y == 15
//                 && event_t.coord.x == 123
//                 && event_t.coord.c_usize() == 0
//                 && event_count > 540
//             {
//                 dbg!(event_t);
//             }
//
//             let a_t = frame_sequence_t.ingest_event(&mut event_t.clone());
//
//             if a_t {
//                 t_frame = frame_sequence_t.pop_next_frame();
//             }
//
//             let event_dt = match input_stream_dt.decode_event() {
//                 Ok(ev) => ev,
//                 Err(_) => {
//                     break;
//                 }
//             };
//             let a_dt = frame_sequence_dt.ingest_event(&mut event_dt.clone());
//
//             if a_dt {
//                 dt_frame = frame_sequence_dt.pop_next_frame();
//
//                 for c in 0..input_stream_dt.plane.c_usize() {
//                     for y in 0..input_stream_dt.plane.h_usize() {
//                         for x in 0..input_stream_dt.plane.w_usize() {
//                             let dt_val =
//                                 dt_frame.clone().unwrap().last().unwrap()[[y, x, c]].unwrap();
//                             let t_val =
//                                 t_frame.clone().unwrap().last().unwrap()[[y, x, c]].unwrap();
//                             assert_eq!(dt_val, t_val);
//                         }
//                     }
//                 }
//                 // assert_eq!(dt_frame.unwrap()[0], t_frame.clone().unwrap()[0]);
//
//                 // let dt_val = dt_frame.unwrap()[0][[0, 0, 0]].unwrap();
//                 // let t_val = t_frame.clone().unwrap()[0][[0, 0, 0]].unwrap();
//                 // assert_eq!(dt_val, t_val);
//             }
//
//             event_count += 1;
//             let last_t = &mut t_tree[[
//                 event_t.coord.y_usize(),
//                 event_t.coord.x_usize(),
//                 event_t.coord.c_usize(),
//             ]];
//
//             let event_t_dt = absolute_event_to_dt_event(event_t, *last_t);
//             *last_t = event_t.delta_t;
//
//             // We already know it's a framed source
//             if *last_t % input_stream_dt.ref_interval != 0 {
//                 *last_t =
//                     ((*last_t / input_stream_dt.ref_interval) + 1) * input_stream_dt.ref_interval;
//             }
//
//             assert_eq!(event_t_dt.coord.x as i32, event_dt.coord.x as i32);
//             assert_eq!(event_t_dt.coord.y as i32, event_dt.coord.y as i32);
//             assert_eq!(event_t_dt.coord.c, event_dt.coord.c);
//             let dt_mig = event_t_dt.delta_t;
//             let dt_gt = event_dt.delta_t;
//             assert_eq!(dt_mig, dt_gt);
//             assert_eq!(event_t_dt.d, event_dt.d);
//         }
//         assert_eq!(event_count, 675693);
//
//         Ok(())
//     }
// }
