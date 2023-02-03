use adder_codec_rs::codec::compressed::blocks::{gen_zigzag_order, Cube, ZigZag, ZIGZAG_ORDER};
use adder_codec_rs::codec::compressed::BLOCK_SIZE_BIG;
use adder_codec_rs::framer::driver::EventCoordless;
use adder_codec_rs::{Coord, Event};
use criterion::{criterion_group, criterion_main, Bencher, BenchmarkId, Criterion};
use criterion_perf_events::Perf;
use perfcnt::linux::HardwareEventType as Hardware;
use perfcnt::linux::PerfCounterBuilderLinux as Builder;

struct Setup {
    cube: Cube,
    event: Event,
    events_for_block_r: Vec<Event>,
    events_for_block_g: Vec<Event>,
    events_for_block_b: Vec<Event>,
}
impl Setup {
    fn new() -> Self {
        let mut events_for_block_r = Vec::new();
        for y in 0..BLOCK_SIZE_BIG {
            for x in 0..BLOCK_SIZE_BIG {
                events_for_block_r.push(Event {
                    coord: Coord {
                        y: y as u16,
                        x: x as u16,
                        c: Some(0),
                    },
                    ..Default::default()
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

fn zig_zag_iter<'a>(
    cube: &'a mut Cube,
    events: Vec<Event>,
    order: &[u16; BLOCK_SIZE_BIG * BLOCK_SIZE_BIG],
) {
    for event in events.iter() {
        assert!(cube.set_event(event.clone()).is_ok());
    }
    // let block_ref = &cube.blocks_r[0];

    let mut zigzag_events = Vec::new();
    let zigzag = ZigZag::new(&cube.blocks_r[0], order);
    let mut iter = zigzag.into_iter();
    for y in 0..BLOCK_SIZE_BIG {
        for x in 0..BLOCK_SIZE_BIG {
            let event = iter.next().unwrap().unwrap();
            zigzag_events.push(event);
        }
    }
}

fn zig_zag_iter2<'a>(cube: &'a mut Cube, events: Vec<Event>) {
    for event in events.iter() {
        assert!(cube.set_event(event.clone()).is_ok());
    }
    // let block_ref = &cube.blocks_r[0];

    let mut zigzag_events = Vec::new();
    let zigzag = ZigZag::new(&cube.blocks_r[0], &ZIGZAG_ORDER);
    let mut iter = zigzag.into_iter();
    for y in 0..BLOCK_SIZE_BIG {
        for x in 0..BLOCK_SIZE_BIG {
            let event = iter.next().unwrap().unwrap();
            zigzag_events.push(event);
        }
    }
}

fn bench_zigzag_iter(c: &mut Criterion) {
    println!("IN BENCH");
    let mut setup = Setup::new();
    let mut cube = setup.cube;
    let mut events = setup.events_for_block_r;
    let zigzag_order = gen_zigzag_order();

    c.bench_function("zigzag iter", |b| {
        b.iter(|| zig_zag_iter(&mut cube, events.clone(), &zigzag_order))
    });

    c.bench_function("zigzag iter 2", |b| {
        b.iter(|| zig_zag_iter(&mut cube, events.clone(), &zigzag_order))
    });
}

fn regular_iter<'a>() {
    let mut setup = Setup::new();
    let mut cube = setup.cube;
    let mut events = setup.events_for_block_r;

    for event in events.iter() {
        assert!(cube.set_event(event.clone()).is_ok());
    }

    let mut out_events = Vec::new();
    let mut iter = cube.blocks_r[0].events.iter();
    for event in &cube.blocks_r[0].events[..] {
        out_events.push(event);
    }
}

fn bench_regular_iter(c: &mut Criterion) {
    println!("IN BENCH");
    c.bench_function("regular iter", |b| b.iter(|| regular_iter()));
}

criterion_group!(
    block,
    bench_zigzag_iter,
    bench_regular_iter,
    // regular_iter_zigzag_insert
);
criterion_main!(block);
