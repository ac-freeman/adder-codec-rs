# ADDER-codec-rs
[![Build Status](https://github.com/ac-freeman/adder-codec-rs/workflows/Rust/badge.svg)](https://github.com/ac-freeman/adder-codec-rs/actions)
[![Documentation](https://docs.rs/adder-codec-rs/badge.svg)](https://docs.rs/adder-codec-rs)
[![Crates.io](https://img.shields.io/crates/v/adder-codec-rs)](https://crates.io/crates/adder-codec-rs)
[![Downloads](https://img.shields.io/crates/dr/adder-codec-rs)](https://crates.io/crates/adder-codec-rs)


Encoder/transcoder/decoder for ADΔER (Address, Decimation, Δt Event Representation) streams. Currently, only implemented for raw (uncompressed) streams. Includes a transcoder for casting framed video into an ADΔER representation in a manner which preserves the temporal synchronicity of the source, but enables many-frame intensity averaging on a per-pixel basis and extremely high dynamic range.

## Setup

If you just want to use the hooks for encoding/decoding ADΔER streams (i.e., not a transcoder for producing the ADΔER events for a given source), then you can include the library by adding the following to your Cargo.toml file:

`adder-codec-rs = {version = "0.1.8", features = ["raw-codec"]}`

If you want to use the provided transcoder(s), then you have to install OpenCV 4.0+ according to the configuration guidelines for [opencv-rust](https://github.com/twistedfall/opencv-rust). Then, include the library in your project as normal:

`adder-codec-rs = "0.1.8"`

## Examples

Clone this repository to run example executables provided in `src/bin`

### Transcode framed video to ADΔER
Run the program `/src/bin/framed_video_to_adder.rs`. You will need to adjust the parameters for the FramedSourceBuilder to suit your needs, as described below.

```
 let mut source =
        // The file path to the video you want to transcode, and the bit depth of the video
        FramedSourceBuilder::new("~/Downloads/excerpt.mp4".to_string(),
                                 SourceCamera::FramedU8)    
        
        // Which frame of the input video to begin your transcode
        .frame_start(1420)  
        
        // Input video is scaled by this amount before transcoding
        .scale(0.5)         
        
        // Must be true for us to do anything with the events
        .communicate_events(true)   
        
        // The file path to store the ADΔER events
        .output_events_filename("~/Downloads/events.adder".to_string())     
        
        // Use color, or convert input video to grayscale first?
        .color(false)       
        
        // Positive and negative contrast thresholds. Larger values = more temporal loss. 0 = nearly no distortion.
        .contrast_thresholds(10, 10)    
        
        // Show a live view of the input frames as they're being transcoded?
        .show_display(true) 
        
        .time_parameters(5000,  // The reference interval: How many ticks does each input frame span?
                         300000,    // Ticks per second. Must equal (reference interval) * (source frame rate)
                         3000000)   // Δt_max: the maximum Δt value for any generated ADΔER event
        .finish();
```

### Direct usage


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

