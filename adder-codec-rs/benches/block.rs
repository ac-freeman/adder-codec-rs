use adder_codec_rs::codec::compressed::blocks::{gen_zigzag_order, Cube, ZigZag, ZIGZAG_ORDER};
use adder_codec_rs::codec::compressed::{BLOCK_SIZE_BIG, BLOCK_SIZE_BIG_AREA};
use std::io::{BufReader, BufWriter, Write};

use adder_codec_rs::codec::compressed::compression::{
    CompressionModelDecoder, CompressionModelEncoder,
};
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
            Some(num) => StdRng::seed_from_u64(num),
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
                    d: rng.gen_range(7..9),
                    delta_t: rng.gen_range(200..300),
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

fn bench_encode_block2(c: &mut Criterion) {
    let setup = Setup::new(Some(473829479));
    let mut cube = setup.cube;
    let events = setup.events_for_block_r;

    for event in events.iter() {
        assert!(cube.set_event(*event).is_ok());
    }

    let mut write_result = Vec::new();
    let out_writer = BufWriter::new(&mut write_result);

    let mut model = CompressionModelEncoder::new(2550, 255, out_writer);

    c.bench_function("encode block", |b| {
        b.iter(|| model.encode_block(&mut cube.blocks_r[0]))
    });

    let mut group = c.benchmark_group("block_2");
    group.significance_level(0.05).sample_size(50);
    group.bench_function("encode MANY blocks", |b| {
        b.iter(|| {
            for _ in 0..100 {
                model.encode_block(&mut cube.blocks_r[0])
            }
        })
    });

    model.flush_encoder();

    let mut writer = model.bitwriter.into_writer();
    writer.flush().unwrap();

    let written = writer.into_inner().unwrap();
    let buf_reader = BufReader::new(&**written);

    let mut context_model = CompressionModelDecoder::new(2550, 255, buf_reader);

    group.bench_function("decode block", |b| {
        b.iter(|| context_model.decode_block(&mut cube.blocks_r[0]))
    });

    // group.bench_function("decode MANY blocks", |b| {
    //     for _ in 0..100 {
    //         b.iter(|| context_model.decode_block(&mut cube.blocks_r[0]))
    //     }
    // });

    // context_model.check_eof();
}

fn bench_encode_block2_semirealistic(c: &mut Criterion) {
    let setup = Setup::new(Some(473829479));
    let mut cube = setup.cube;
    let events = setup.events_for_block_g;

    for event in events.iter() {
        assert!(cube.set_event(*event).is_ok());
    }

    let mut write_result = Vec::new();
    let out_writer = BufWriter::new(&mut write_result);

    let mut model = CompressionModelEncoder::new(2550, 255, out_writer);

    c.bench_function("encode block semirealistic", |b| {
        b.iter(|| model.encode_block(&mut cube.blocks_g[0]))
    });

    let mut group = c.benchmark_group("block_2");
    group.significance_level(0.05).sample_size(50);
    group.bench_function("encode MANY blocks semirealistic", |b| {
        b.iter(|| {
            for _ in 0..100 {
                model.encode_block(&mut cube.blocks_g[0])
            }
        })
    });

    model.flush_encoder();

    let mut writer = model.bitwriter.into_writer();
    writer.flush().unwrap();

    let written = writer.into_inner().unwrap();
    let buf_reader = BufReader::new(&**written);

    let mut context_model = CompressionModelDecoder::new(2550, 255, buf_reader);

    group.bench_function("decode block semirealistic", |b| {
        b.iter(|| context_model.decode_block(&mut cube.blocks_g[0]))
    });

    // group.bench_function("decode MANY blocks", |b| {
    //     for _ in 0..100 {
    //         b.iter(|| context_model.decode_block(&mut cube.blocks_g[0]))
    //     }
    // });
    //
    // context_model.check_eof();
}

criterion_group!(
    block_2,
    bench_encode_block2,
    bench_encode_block2_semirealistic
);
criterion_main!(block_2);
