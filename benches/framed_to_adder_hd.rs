use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use criterion_perf_events::Perf;
use perfcnt::linux::HardwareEventType as Hardware;
use perfcnt::linux::PerfCounterBuilderLinux as Builder;

use adder_codec_rs::transcoder::source::framed_source::FramedSourceBuilder;
use adder_codec_rs::transcoder::source::video::Source;
use adder_codec_rs::utils::simulproc::{SimulProcArgs, SimulProcessor};
use adder_codec_rs::SourceCamera::FramedU8;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::thread::sleep;
use std::time::Duration;

fn bench_simul_proc_dark() {
    let d = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let manifest_path_str = d.as_path().to_str().unwrap().to_owned();

    let args: SimulProcArgs = SimulProcArgs {
        color_input: 0,
        tps: 120000,
        fps: 24,
        ref_time: 5000,
        delta_t_max: 120000,
        frame_count_max: 0,
        frame_idx_start: 0,
        show_display: 0,
        input_filename: manifest_path_str.clone() + "/tests/samples/lake_scaled_hd_crop.mp4",
        output_events_filename: manifest_path_str.clone()
            + "/tests/samples/TEST_lake_scaled_hd_crop.adder",
        output_raw_video_filename: manifest_path_str + "/tests/samples/TEST_lake_scaled_hd_crop",
        scale: 1.0,
        c_thresh_pos: 0,
        c_thresh_neg: 0,
        thread_count: 1, // Multithreading causes some issues in testing
    };
    let mut source_builder = FramedSourceBuilder::new(args.input_filename, FramedU8)
        .frame_start(args.frame_idx_start)
        .scale(args.scale)
        .communicate_events(true)
        .color(args.color_input != 0)
        .contrast_thresholds(args.c_thresh_pos, args.c_thresh_neg)
        .show_display(args.show_display != 0)
        .time_parameters(args.tps, args.delta_t_max);
    if !args.output_events_filename.is_empty() {
        source_builder = source_builder.output_events_filename(args.output_events_filename);
    }
    let source = source_builder.finish();
    let ref_time = source.get_ref_time();

    let mut simul_processor = SimulProcessor::new::<u8>(
        source,
        ref_time,
        args.tps,
        args.fps,
        args.output_raw_video_filename.as_str(),
        args.frame_count_max as i32,
        1,
    );

    simul_processor.run().unwrap();
    sleep(Duration::from_secs(5));

    let output_path = "./tests/samples/TEST_lake_scaled_hd_crop";
    assert_eq!(
        fs::metadata(output_path).unwrap().len()
            % (simul_processor.source.get_video().width as u64
                * simul_processor.source.get_video().height as u64),
        0
    );

    let output = if !cfg!(target_os = "windows") {
        Command::new("sh")
            .arg("-c")
            .arg("cmp ./tests/samples/TEST_lake_scaled_hd_crop ./tests/samples/lake_scaled_out")
            .output()
            .expect("failed to execute process")
    } else {
        fs::remove_file(output_path).unwrap();
        return;
    };
    // println!("{}", String::from_utf8(output.stdout.clone()).unwrap());

    // Note the file might be larger than that given in ./tests/samples, if the method for
    // framing generates more frames at the end than the original method used. This assertion
    // should still pass if all the frames before that are identical.
    assert_eq!(output.stdout.len(), 0);
    fs::remove_file(output_path).unwrap();

    let output_path = "./tests/samples/TEST_lake_scaled_hd_crop.adder";
    fs::remove_file(output_path).unwrap();
}

fn bench(c: &mut Criterion<Perf>) {
    let mut group = c.benchmark_group("simul_proc");

    let fibo_arg = 30;
    group.bench_function(BenchmarkId::new("simul_proc_dark", "dark"), |b| {
        b.iter(|| bench_simul_proc_dark())
    });

    group.finish()
}

criterion_group!(
    name = framed_to_adder_hd;
    config = Criterion::default().with_measurement(Perf::new(Builder::from_hardware_event(Hardware::CacheMisses))).sample_size(10);
    targets = bench
);
criterion_main!(framed_to_adder_hd);
