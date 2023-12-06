# Contributing

## Run the tests

        cargo test

## Run the bench tests

        cargo bench

## Run the fuzz tests

(requires [cargo-fuzz](https://github.com/rust-fuzz/cargo-fuzz))

        cargo fuzz run fuzz_target_1

## Run the examples

        cargo run --example=${EXAMPLE}
