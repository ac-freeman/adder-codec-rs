[![Crates.io](https://img.shields.io/crates/v/adder-to-dvs)](https://crates.io/crates/adder-to-dvs)
[![Downloads](https://img.shields.io/crates/dr/adder-to-dvs)](https://crates.io/crates/adder-to-dvs)

This program transcodes an ADΔER file to DVS events in a human-readable text representation.
Performance is fast. The resulting DVS stream is visualized during the transcode and written
out as an mp4 file.

Install: `cargo install adder-to-dvs`

Run: `adder-to-dvs -- --input "/mnt/tmp/tmp_events.adder" --output-text "/home/andrew/Downloads/adder.dvs" --output-video "/home/andrew/Downloads/adder.dvs.mp4"`
