# ADDER-codec-rs
[![Build Status](https://github.com/ac-freeman/adder-codec-rs/workflows/Rust/badge.svg)](https://github.com/ac-freeman/adder-codec-rs/actions)
[![Documentation](https://docs.rs/adder-codec-rs/badge.svg)](https://docs.rs/adder-codec-rs)
[![Crates.io](https://img.shields.io/crates/v/adder-codec-rs)](https://crates.io/crates/adder-codec-rs)
[![Downloads](https://img.shields.io/crates/dr/adder-codec-rs)](https://crates.io/crates/adder-codec-rs)


Encoder/transcoder/decoder for ADΔER (Address, Decimation, Δt Event Representation) streams. Currently, only implemented for raw (uncompressed) streams. Includes a transcoder for casting framed video into an ADΔER representation in a manner which preserves the temporal synchronicity of the source, but enables many-frame intensity averaging on a per-pixel basis and extremely high dynamic range.

### Setup

If you just want to use the hooks for encoding/decoding ADΔER streams (i.e., not a transcoder for producing the ADΔER events for a given source), then you can include the library by adding the following to your Cargo.toml file:

`adder-codec-rs = {version = "0.1.4", features = ["raw-codec"]}`

If you want to use the provided transcoder(s), then you have to install OpenCV 4.0+ according to the configuration guidelines for [opencv-rust](https://github.com/twistedfall/opencv-rust). Then, include the library in your project as normal:

`adder-codec-rs = "0.1.4"`

### Examples

Example executables are provided in `src/bin` for both transcoding framed video to  ADΔER, and for reconstructing a framed representation from ADΔER. More thorough examples are to come.

### Direct Usage


Encode a raw stream:
```
let mut stream: RawStream = Codec::new();
match stream.open_writer("/path/to/file") {
    Ok(_) => {}
    Err(e) => {panic!("{}", e)}
};
stream.encode_header(500, 200, 50000, 5000, 50000, 1);

let event: Event = Event {
        coord: Coord {
            x: 10,
            y: 30,
            c: None
        },
        d: 5,
        delta_t: 1000
    };
let events = vec![event, event, event]; // Encode three identical events, for brevity's sake
stream.encode_events(&events);
stream.close_writer();
```

Read a raw stream:

```
let mut stream: RawStream = Codec::new();
stream.open_reader(args.input_filename.as_str())?;
stream.decode_header();
match self.stream.decode_event() {
    Ok(event) => {
        // Do something with the event
    }
    Err(_) => panic!("Couldn't read event :("),
};
stream.close_reader();
```

