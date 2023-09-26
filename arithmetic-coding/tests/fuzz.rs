use fenwick_model::simple::FenwickModel;

mod common;

#[test]
fn round_trip() {
    let model = FenwickModel::builder(256, 1 << 20).build();
    let bytes: &[u8] = &[220, 255, 255];
    let input: Vec<usize> = bytes.iter().copied().map(usize::from).collect();

    common::round_trip(model, &input);
}
