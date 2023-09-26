#![no_main]
use fenwick_model::simple::FenwickModel;
use libfuzzer_sys::fuzz_target;

mod round_trip;

fuzz_target!(|data: &[u8]| {
    let model = FenwickModel::builder(256, 1 << 20).build();
    let input: Vec<usize> = data.into_iter().copied().map(usize::from).collect();

    round_trip::round_trip(model, input);
});
