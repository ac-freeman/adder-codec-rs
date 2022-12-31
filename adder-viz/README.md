# adder-viz
[![Crates.io](https://img.shields.io/crates/v/adder-viz)](https://crates.io/crates/adder-viz)
[![Downloads](https://img.shields.io/crates/dr/adder-viz)](https://crates.io/crates/adder-viz)

A GUI project to make it easier to tune the parameters of ADΔER transcoding.

![](https://github.com/ac-freeman/adder-codec-rs/blob/main/adder-viz/examples/screenshot.png)

# Dependencies

You may need to install the Bevy dependencies described [here](https://bevyengine.org/learn/book/getting-started/setup/) and install OpenCV as described [here](https://github.com/twistedfall/opencv-rust).

# Installation

`cargo install adder-viz`

# Usage

Run `adder-viz` in the terminal and the above window will open. Drag and drop your video of choice from a file manager, and the ADΔER transcode process will begin automatically. Currently, it only supports .mp4 video sources and .aedat4 DAVIS 346 camera sources. Some parameter adjustments, such as the video scale, require the transcode process to be relaunched, which causes a noticeable slowdown in the UI for a moment. The program can also playback `.adder` files, which you can even generate on the Transcode tab.
