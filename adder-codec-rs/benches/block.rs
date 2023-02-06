use adder_codec_rs::codec::compressed::blocks::{gen_zigzag_order, Cube, ZigZag, ZIGZAG_ORDER};
use adder_codec_rs::codec::compressed::{BLOCK_SIZE_BIG, BLOCK_SIZE_BIG_AREA};
use arithmetic_coding::Encoder;
use bitstream_io::{BigEndian, BitWrite, BitWriter};

use adder_codec_rs::codec::compressed::compression::BlockIntraPredictionContextModel;
use adder_codec_rs::{Coord, Event};
use criterion::{criterion_group, criterion_main, Criterion};
use rand::prelude::StdRng;
use rand::{Rng, SeedableRng};

struct Setup {
    cube: Cube,
    event: Event,
    events_for_block_r: Vec<Event>,
    events_for_block_g: Vec<Event>,
    events_for_block_b: Vec<Event>,
}
impl Setup {
    fn new(seed: Option<u64>) -> Self {
        let mut rng = match seed {
            None => StdRng::from_rng(rand::thread_rng()).unwrap(),
            Some(num) => StdRng::seed_from_u64(42),
        };
        //
        let mut events_for_block_r = Vec::new();
        for y in 0..BLOCK_SIZE_BIG {
            for x in 0..BLOCK_SIZE_BIG {
                events_for_block_r.push(Event {
                    coord: Coord {
                        y: y as u16,
                        x: x as u16,
                        c: Some(0),
                    },

                    d: rng.gen_range(0..20),
                    delta_t: rng.gen_range(1..2550),
                });
            }
        }

        let mut events_for_block_g = Vec::new();
        for y in 0..BLOCK_SIZE_BIG {
            for x in 0..BLOCK_SIZE_BIG {
                events_for_block_g.push(Event {
                    coord: Coord {
                        y: y as u16,
                        x: x as u16,
                        c: Some(1),
                    },
                    ..Default::default()
                });
            }
        }

        let mut events_for_block_b = Vec::new();
        for y in 0..BLOCK_SIZE_BIG {
            for x in 0..BLOCK_SIZE_BIG {
                events_for_block_b.push(Event {
                    coord: Coord {
                        y: y as u16,
                        x: x as u16,
                        c: Some(2),
                    },
                    ..Default::default()
                });
            }
        }

        Self {
            cube: Cube::new(0, 0, 0),
            event: Event {
                coord: Coord {
                    x: 0,
                    y: 0,
                    c: Some(0),
                },
                d: 7,
                delta_t: 100,
            },
            events_for_block_r,
            events_for_block_g,
            events_for_block_b,
        }
    }
}

fn zig_zag_iter(cube: &mut Cube, events: Vec<Event>, order: &[u16; BLOCK_SIZE_BIG_AREA]) {
    for event in events.iter() {
        assert!(cube.set_event(*event).is_ok());
    }

    let mut zigzag_events = Vec::new();
    let zigzag = ZigZag::new(&cube.blocks_r[0], order);
    let mut iter = zigzag;
    for _y in 0..BLOCK_SIZE_BIG {
        for _x in 0..BLOCK_SIZE_BIG {
            let event = iter.next().unwrap().unwrap();
            zigzag_events.push(event);
        }
    }
}

fn zig_zag_iter2(cube: &mut Cube, events: Vec<Event>) {
    for event in events.iter() {
        assert!(cube.set_event(*event).is_ok());
    }

    let mut zigzag_events = Vec::new();
    let zigzag = ZigZag::new(&cube.blocks_r[0], &ZIGZAG_ORDER);
    let mut iter = zigzag;
    for _y in 0..BLOCK_SIZE_BIG {
        for _x in 0..BLOCK_SIZE_BIG {
            let event = iter.next().unwrap().unwrap();
            zigzag_events.push(event);
        }
    }
}

fn bench_zigzag_iter(c: &mut Criterion) {
    println!("IN BENCH");
    let setup = Setup::new(None);
    let mut cube = setup.cube;
    let events = setup.events_for_block_r;
    let zigzag_order = gen_zigzag_order();

    c.bench_function("zigzag iter", |b| {
        b.iter(|| zig_zag_iter(&mut cube, events.clone(), &zigzag_order))
    });

    c.bench_function("zigzag iter 2", |b| {
        b.iter(|| zig_zag_iter2(&mut cube, events.clone()))
    });
}

fn bench_zigzag_iter_alloc(c: &mut Criterion) {
    println!("IN BENCH");
    let setup = Setup::new(None);
    let mut cube = setup.cube;
    let events = setup.events_for_block_r;

    c.bench_function("zigzag iter with alloc", |b| {
        let zigzag_order = gen_zigzag_order();
        b.iter(|| zig_zag_iter(&mut cube, events.clone(), &zigzag_order))
    });
}

fn regular_iter<'a>() {
    let setup = Setup::new(None);
    let mut cube = setup.cube;
    let events = setup.events_for_block_r;

    for event in events.iter() {
        assert!(cube.set_event(*event).is_ok());
    }

    let mut out_events = Vec::new();
    let _iter = cube.blocks_r[0].events.iter();
    for event in &cube.blocks_r[0].events[..] {
        out_events.push(event);
    }
}

fn bench_regular_iter(c: &mut Criterion) {
    c.bench_function("regular iter", |b| b.iter(regular_iter));
}

fn bench_encode_block(c: &mut Criterion) {
    let mut context_model = BlockIntraPredictionContextModel::new(2550);
    let setup = Setup::new(Some(473829479));
    let mut cube = setup.cube;
    let events = setup.events_for_block_r;

    for event in events.iter() {
        assert!(cube.set_event(*event).is_ok());
    }

    let mut out_writer = BitWriter::endian(Vec::new(), BigEndian);

    c.bench_function("encode block", |b| {
        b.iter(|| context_model.encode_block(&mut cube.blocks_r[0], &mut out_writer))
    });

    let writer: &[u8] = &*out_writer.into_writer();

    c.bench_function("decode block", |b| {
        b.iter(|| context_model.decode_block(&mut cube.blocks_r[0], writer))
    });
}

fn bench_encode_event(c: &mut Criterion) {
    let mut context_model = BlockIntraPredictionContextModel::new(2550);
    let setup = Setup::new(Some(473829479));
    let mut cube = setup.cube;
    let events = setup.events_for_block_r;

    for event in events.iter() {
        assert!(cube.set_event(*event).is_ok());
    }

    let mut d_writer = BitWriter::endian(Vec::new(), BigEndian);
    let mut d_encoder = Encoder::new(context_model.d_model.clone(), &mut d_writer); // Todo: shouldn't clone models unless at new AVU time point, ideally...
    let mut dt_writer = BitWriter::endian(Vec::new(), BigEndian);
    let mut dt_encoder = Encoder::new(context_model.delta_t_model.clone(), &mut dt_writer);

    context_model.encode_event(Some(&events[0].into()), &mut d_encoder, &mut dt_encoder);

    c.bench_function("encode event", |b| {
        b.iter(|| {
            context_model.encode_event(Some(&events[1].into()), &mut d_encoder, &mut dt_encoder)
        })
    });

    let mut context_model = BlockIntraPredictionContextModel::new(2550);
    let mut d_writer = BitWriter::endian(Vec::new(), BigEndian);
    let mut d_encoder = Encoder::new(context_model.d_model.clone(), &mut d_writer); // Todo: shouldn't clone models unless at new AVU time point, ideally...
    let mut dt_writer = BitWriter::endian(Vec::new(), BigEndian);
    let mut dt_encoder = Encoder::new(context_model.delta_t_model.clone(), &mut dt_writer);

    c.bench_function("encode block of events", |b| {
        b.iter(|| {
            let zigzag = ZigZag::new(&cube.blocks_r[0], &ZIGZAG_ORDER);
            for event in zigzag {
                context_model.encode_event(event, &mut d_encoder, &mut dt_encoder);
            }
        })
    });
}

fn bench_encode_event2(c: &mut Criterion) {
    let setup = Setup::new(Some(473829479));
    let mut cube = setup.cube;
    let events = setup.events_for_block_r;

    for event in events.iter() {
        assert!(cube.set_event(*event).is_ok());
    }

    c.bench_function("write OUT encoded events", |b| {
        b.iter(|| {
            let mut context_model = BlockIntraPredictionContextModel::new(2550);
            let mut d_writer = BitWriter::endian(Vec::new(), BigEndian);
            let mut d_encoder = Encoder::new(context_model.d_model.clone(), &mut d_writer); // Todo: shouldn't clone models unless at new AVU time point, ideally...
            let mut dt_writer = BitWriter::endian(Vec::new(), BigEndian);
            let mut dt_encoder = Encoder::new(context_model.delta_t_model.clone(), &mut dt_writer);
            let mut out_writer = BitWriter::endian(Vec::new(), BigEndian);

            let zigzag = ZigZag::new(&cube.blocks_r[0], &ZIGZAG_ORDER);
            for event in zigzag {
                context_model.encode_event(event, &mut d_encoder, &mut dt_encoder);
            }

            d_encoder.flush().unwrap();
            d_writer.byte_align().unwrap();
            dt_encoder.flush().unwrap();
            dt_writer.byte_align().unwrap();

            let d = d_writer.into_writer();
            /* The compressed length of the d residuals
            should always be representable in 2 bytes. Write that signifier as a u16.
             */
            let d_len_bytes = (d.len() as u16).to_be_bytes();
            out_writer.write_bytes(&d_len_bytes).unwrap();
            out_writer.write_bytes(&d).unwrap();
            out_writer.write_bytes(&dt_writer.into_writer()).unwrap();
        })
    });
}

criterion_group!(
    block,
    bench_zigzag_iter,
    bench_regular_iter,
    bench_zigzag_iter_alloc,
    bench_encode_block,
    bench_encode_event,
    bench_encode_event2,
);
criterion_main!(block);
