# adder-viz
[![Crates.io](https://img.shields.io/crates/v/adder-viz)](https://crates.io/crates/adder-viz)
[![Downloads](https://img.shields.io/crates/dr/adder-viz)](https://crates.io/crates/adder-viz)

A GUI project to make it easier to tune the parameters of ADΔER transcoding.

![](https://github.com/ac-freeman/adder-codec-rs/blob/main/adder-viz/examples/screenshot.png)

# Dependencies

You may need to install the Bevy dependencies described [here](https://bevyengine.org/learn/book/getting-started/setup/).

If you want to transcode from DVS/DAVIS, we depend on [davis-EDI-rs](https://crates.io/crates/davis-edi-rs). For that (for now), you have to install OpenCV as described [here](https://github.com/twistedfall/opencv-rust).

# Installation

`cargo install adder-viz`

Install with DVS/DAVIS support:

`cargo install adder-viz -F "open-cv"`

Install with source-modeled compression support:

`cargo install adder-viz -F "compression"`

# Usage

Run `adder-viz` in the terminal and the above window will open. Drag and drop your video of choice from a file manager, and the ADΔER transcode process will begin automatically. Currently, it only supports .mp4 video sources, .aedat4 DAVIS 346 camera sources, and DAVIS 346 camera sources connected via Unix sockets. Some parameter adjustments, such as the video scale, require the transcode process to be relaunched, which causes a noticeable slowdown in the UI for a moment. The program can also playback `.adder` files, which you can even generate on the Transcode tab.
