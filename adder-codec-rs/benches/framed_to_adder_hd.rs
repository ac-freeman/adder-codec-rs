use criterion::{criterion_group, criterion_main, Bencher, BenchmarkId, Criterion};
use criterion_perf_events::Perf;
use perfcnt::linux::HardwareEventType as Hardware;
use perfcnt::linux::PerfCounterBuilderLinux as Builder;

use adder_codec_rs::utils::simulproc::{SimulProcArgs, SimulProcessor};

use std::fs;

use std::path::PathBuf;

use adder_codec_rs::transcoder::source::framed::Framed;
use adder_codec_rs::transcoder::source::video::VideoBuilder;
use adder_codec_rs::utils::viz::download_file;
use std::thread::sleep;
use std::time::Duration;

fn simul_proc(video_path: &str, scale: f64, thread_count: u8, _chunk_rows: usize) {
    let d = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let manifest_path_str = d.as_path().to_str().unwrap().to_owned();

    let args: SimulProcArgs = SimulProcArgs {
        args_filename: "".to_string(),
        color_input: false,
        ref_time: 5000,
        delta_t_max: 120000,
        frame_count_max: 300,
        frame_idx_start: 0,
        show_display: false,
        input_filename: video_path.to_string(),
        output_events_filename: "".parse().unwrap(),
        output_raw_video_filename: manifest_path_str + "/benches/run/bench_out",
        scale,
        c_thresh_pos: 0,
        c_thresh_neg: 0,
        thread_count, // Multithreading causes some issues in testing
    };
    let source = Framed::new(args.input_filename, args.color_input, args.scale)
        .unwrap()
        // TODO: chunk_rows back
        .frame_start(args.frame_idx_start)
        .unwrap()
        .contrast_thresholds(args.c_thresh_pos, args.c_thresh_neg)
        .show_display(args.show_display)
        .auto_time_parameters(args.ref_time, args.delta_t_max);

    let ref_time = source.get_ref_time();

    let mut simul_processor = SimulProcessor::new::<u8>(
        source,
        ref_time,
        args.output_raw_video_filename.as_str(),
        args.frame_count_max as i32,
        thread_count as usize,
    )
    .unwrap();

    simul_processor.run().unwrap();
    sleep(Duration::from_secs(2));

    let output_path = "./benches/run/bench_out";
    fs::remove_file(output_path).unwrap();
}

fn bench_simul_proc_dark() {
    let d = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let manifest_path_str = d.as_path().to_str().unwrap().to_owned();
    let path_str = manifest_path_str + "/tests/samples/lake_scaled_hd_crop.mp4";
    simul_proc(&path_str, 1.0, 1, 4);
}

fn bench_simul_proc_drop(scale: f64, chunk_rows: usize) {
    let path_str = "./benches/run/drop.mp4";
    let video_url = "https://www.pexels.com/video/2603664/download/";
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(download_file(path_str, video_url))
        .expect("TODO: panic message");
    simul_proc(path_str, scale, 16, chunk_rows);
}

fn bench(c: &mut Criterion<Perf>) {
    let mut group = c.benchmark_group("simul_proc");

    for r in [1, 32, 64, 128, 256, 512].iter() {
        // for i in [0.1_f64, 0.5, 1.0].iter() {
        {
            let i = &0.5_f64;
            let function_name = "chunks ".to_owned() + r.to_string().as_str() + ";";
            let id = BenchmarkId::new(function_name, i);

            group.bench_with_input(id, i, |b: &mut Bencher<Perf>, i| {
                b.iter(|| bench_simul_proc_drop(*i, *r))
            });
        }
    }

    group.finish()
}

criterion_group!(
    name = framed_to_adder_hd;
    config = Criterion::default().with_measurement(Perf::new(Builder::from_hardware_event(Hardware::CacheMisses))).sample_size(10);
    targets = bench
);
criterion_main!(framed_to_adder_hd);
