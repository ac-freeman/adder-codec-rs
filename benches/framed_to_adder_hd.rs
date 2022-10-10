use criterion::{black_box, criterion_group, criterion_main, Bencher, BenchmarkId, Criterion};
use criterion_perf_events::Perf;
use perfcnt::linux::HardwareEventType as Hardware;
use perfcnt::linux::PerfCounterBuilderLinux as Builder;

use adder_codec_rs::transcoder::source::framed_source::FramedSourceBuilder;
use adder_codec_rs::transcoder::source::video::Source;
use adder_codec_rs::utils::simulproc::{SimulProcArgs, SimulProcessor};
use adder_codec_rs::SourceCamera::FramedU8;
use std::fs;
use std::fs::File;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread::sleep;
use std::time::Duration;

#[allow(dead_code)]
async fn download_file(
    store_path: &str,
    video_url: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Download the drop.mp4 video example, if you don't already have it
    let path_str = store_path;
    if !Path::new(path_str).exists() {
        let resp = reqwest::get(video_url).await?;
        let mut file_out = File::create(path_str).expect("Could not create file on disk");
        let mut data_in = Cursor::new(resp.bytes().await?);
        std::io::copy(&mut data_in, &mut file_out)?;
    }
    Ok(())
}

fn simul_proc(video_path: &str, scale: f64, thread_count: u8, chunk_rows: usize) {
    let d = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let manifest_path_str = d.as_path().to_str().unwrap().to_owned();

    let args: SimulProcArgs = SimulProcArgs {
        color_input: 0,
        tps: 120000,
        fps: 24,
        ref_time: 5000,
        delta_t_max: 120000,
        frame_count_max: 300,
        frame_idx_start: 0,
        show_display: 0,
        input_filename: video_path.to_string(),
        output_events_filename: "".parse().unwrap(),
        output_raw_video_filename: manifest_path_str + "/benches/run/bench_out",
        scale,
        c_thresh_pos: 0,
        c_thresh_neg: 0,
        thread_count, // Multithreading causes some issues in testing
    };
    let mut source_builder = FramedSourceBuilder::new(args.input_filename, FramedU8)
        .chunk_rows(chunk_rows)
        .frame_start(args.frame_idx_start)
        .scale(args.scale)
        .communicate_events(true)
        .color(args.color_input != 0)
        .contrast_thresholds(args.c_thresh_pos, args.c_thresh_neg)
        .show_display(args.show_display != 0)
        .time_parameters(args.tps, args.delta_t_max);

    let source = source_builder.finish();
    let ref_time = source.get_ref_time();

    let mut simul_processor = SimulProcessor::new::<u8>(
        source,
        ref_time,
        args.tps,
        args.fps,
        args.output_raw_video_filename.as_str(),
        args.frame_count_max as i32,
        thread_count as usize,
    );

    simul_processor.run().unwrap();
    sleep(Duration::from_secs(2));

    let output_path = "./benches/run/bench_out";
    fs::remove_file(output_path).unwrap();
}

fn bench_simul_proc_dark() {
    let d = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let manifest_path_str = d.as_path().to_str().unwrap().to_owned();
    let path_str = manifest_path_str.clone() + "/tests/samples/lake_scaled_hd_crop.mp4";
    simul_proc(&path_str, 1.0, 1, 4);
}

fn bench_simul_proc_drop(scale: f64, chunk_rows: usize) {
    let path_str = "./benches/run/drop.mp4";
    let video_url = "https://www.pexels.com/video/2603664/download/";
    let mut rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(download_file(path_str, video_url))
        .expect("TODO: panic message");
    simul_proc(path_str, scale, 16, chunk_rows);
}

fn bench(c: &mut Criterion<Perf>) {
    let mut group = c.benchmark_group("simul_proc");

    for r in [1, 32, 64, 128, 256, 512].iter() {
        // for i in [0.1_f64, 0.5, 1.0].iter() {
        for i in [0.5_f64].iter() {
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
