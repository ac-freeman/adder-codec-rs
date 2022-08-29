# ADDER-codec-rs
[![Build Status](https://github.com/ac-freeman/adder-codec-rs/workflows/Rust/badge.svg)](https://github.com/ac-freeman/adder-codec-rs/actions)
[![Documentation](https://docs.rs/adder-codec-rs/badge.svg)](https://docs.rs/adder-codec-rs)
[![Crates.io](https://img.shields.io/crates/v/adder-codec-rs)](https://crates.io/crates/adder-codec-rs)
[![Downloads](https://img.shields.io/crates/dr/adder-codec-rs)](https://crates.io/crates/adder-codec-rs)


Encoder/transcoder/decoder for ADΔER (Address, Decimation, Δt Event Representation) streams. Currently, only implemented for raw (uncompressed) streams. Includes a transcoder for casting framed video into an ADΔER representation in a manner which preserves the temporal synchronicity of the source, but enables many-frame intensity averaging on a per-pixel basis and extremely high dynamic range.

Source 8-bit image frame with shadows boosted ([source video](https://www.pexels.com/video/river-between-trees-2126081/))      |  Frame reconstructed from ADΔER events, generated from 48 input frames, with shadows boosted. Note the greater dynamic range and temporal denoising in the shadows.
:-------------------------:|:-------------------------:
![](https://github.com/ac-freeman/adder-codec-rs/blob/main/source_frame_0.jpg)  |  ![](https://github.com/ac-freeman/adder-codec-rs/blob/main/out_16bit_2_c10.jpg)

# Background

The ADΔER (pronounced "adder") representation is inspired by the ASINT camera design by Singh et al. It aims to help us move away from thinking about video in terms of fixed sample rates and frames, and to provide a one-size-fits-all ("narrow waist") method for representing intensity information **_asynchronously_**.

 <a href="http://www.youtube.com/watch?v=yfzwn5PrMpw"><img align="right" width="384" height="288" src="https://yt-embed.herokuapp.com/embed?v=yfzwn5PrMpw"></a>

Under the ASINT model, a pixel $\langle x,y\rangle$ continuously integrates light, firing an ADΔER event $\langle x,y,D,\Delta t\rangle$ when it accumulates $2^D$ intensity units (e.g., photons), where $D$ is a _decimation threshold_ and $\Delta t$ is the time elapsed since the pixel last fired an event. we measure $t$ in clock “ticks,'' where the granularity of a clock tick length is user-adjustable. In a raw ADΔER stream, the events are time-ordered and spatially interleaved. An ADΔER event directly specifies an intensity, $I$, by $I \approx \frac{2^D}{\Delta t}$. The key insight of the ASINT model is _the dynamic, pixelwise control of_ $D$. Lowering $D$ for a pixel will increase its event rate, while raising $D$ will decrease its event rate. With this multi-faceted $D$ control, we can ensure that pixel sensitivities are well-tuned to scene dynamics.

Practically speaking, it's most useful to think about ADΔER in reference to the source data type. In the current iteration of this package, I only provide tools for transcoding framed video to ADΔER, but in the future I will release tools for transcoding data from real-world event cameras (e.g., DVS and DAVIS).

In the context of framed video, ADΔER allows us to have multi-frame intensity _averaging_ for stable (unchanging) regions of a scene. This can function both to denoise the video and enable higher dynamic range, all while preserving the temporal synchronicity of the source. See the info on [simultaneous transcoding](#Simultaneously-transcode-framed-video-_to_-ADΔER-events-and-_back_-to-framed-video) to quickly test this out!

# Setup

If you just want to use the hooks for encoding/decoding ADΔER streams (i.e., not a transcoder for producing the ADΔER events for a given source), then you can include the library by adding the following to your Cargo.toml file:

`adder-codec-rs = {version = "0.1.11", features = ["raw-codec"]}`

If you want to use the provided transcoder(s), then you have to install OpenCV 4.0+ according to the configuration guidelines for [opencv-rust](https://github.com/twistedfall/opencv-rust). Then, include the library in your project as normal:

`adder-codec-rs = "0.1.11"`

# Examples

Clone this repository to run example executables provided in `src/bin`

## Transcode framed video _to_ ADΔER events
We can transcode an arbitrary framed video to the ADΔER format. Run the program `/src/bin/framed_video_to_adder.rs`. You will need to adjust the parameters for the FramedSourceBuilder to suit your needs, as described below.

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

## Generate framed video _from_ ADΔER events

We can also transform our ADΔER file back into a framed video, so we can easily view the effects of our transcode parameters. Run the program `/src/bin/events_to_instantaneous_frames.rs`. You will need to set the `input_path` to point to an ADΔER file, and the `output_path` to where you want the resulting framed video to be. This output file is in a raw pixel format for encoding with FFmpeg: either `gray` or `bgr24` (if in color), assuming that we have constructed a `FrameSequence<u8>`. Other formats can be encoded, e.g. with `FrameSequence<u16>`, `FrameSequence<f64>`, etc.

To take our raw frame data and encode it in a standard format, we can use an FFmpeg command as follows:
```
ffmpeg -f rawvideo -pix_fmt gray -s:v 960x540 -r 60 -i ./events.adder -crf 0 -c:v libx264 -y ./events_to_framed.mp4
```

## Simultaneously transcode framed video _to_ ADΔER events and _back_ to framed video

This is the most thorough example, complete with an argument parser so you don't have to edit the code. Run the program `/src/bin/transcode_and_frame_simultaneous.rs`, like this:

```
cargo run --release --bin transcode_and_frame_simultaneous -- 
    --scale 1.0 
    --input-filename "/path/to/video"
    --output-raw-video-filename "/path/to/output_video"
    --c-thresh-pos 10
    --c-thresh-neg 10
```

The program will re-frame the ADΔER events as they are being generated, without having to write them out to a file. This lets you quickly experiment with different values for `c_thresh_pos`, `c_thresh_neg`, `ref_time`, `delta_t_max`, and `tps`, to see what effect they have on the output.

## Inspect an ADΔER file

Want to quickly view the metadata for an ADΔER file? Just execute:

```
cargo run --release --bin adderinfo -- -i /path/to/file.adder -d
```

The `-d` flag enables the calculation of dynamic the ADΔER file's dynamic range. This can take a while, since each event must be decoded to find the event with the maximum intensity and the minimum intensity. Example output:

```
Dimensions
	Width: 960
	Height: 540
	Color channels: 3
Source camera: FramedU8 - Framed video with 8-bit pixel depth, unsigned integer
ADΔER transcoder parameters
	Codec version: 1
	Ticks per second: 120000
	Reference ticks per source interval: 5000
	Δt_max: 240000
File metadata
	File size: 1114272056
	Header size: 29
	ADΔER event count: 111427201
	Events per pixel: 214
Dynamic range
	Theoretical range:
		114 dB (power)
		37 bits
	Realized range:
		27 dB (power)
		9 bits
```

## Direct usage


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

