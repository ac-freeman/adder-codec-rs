"# adder-viz
[![Crates.io](https://img.shields.io/crates/v/adder-viz)](https://crates.io/crates/adder-viz)
[![Downloads](https://img.shields.io/crates/dr/adder-viz)](https://crates.io/crates/adder-viz)

A GUI project to make it easier to tune the parameters of ADΔER transcoding.

![](https://github.com/ac-freeman/adder-codec-rs/blob/main/adder-viz/examples/screenshot.png)


First, you need to get the necessary dependencies installed. These instructions assume you're running a flavor of Debian Linux. It should also work within the Windows Subsystem for Linux, which now supports graphical display.

If you don't want to install these dependencies, you can use the VirtualBox image provided [here](https://drive.google.com/drive/folders/1pCpvvyvwT3sb6fkV4uwePpP7mQN5o-sL?usp=sharing). The link provides instructions for running the virtual machine.

### Install Rust

Use the official instructions [here](https://www.rust-lang.org/tools/install) to install Rust.

### Dependencies

Audio/Video
```
sudo apt-get install -y --fix-missing libodbccr2 libodbc2 libssl-dev alsa-utils libasound2-dev portaudio19-dev build-essential libpulse-dev libdbus-1-dev libudev-dev libatk1.0-dev libgtk-3-dev libavfilter-dev libavdevice-dev ffmpeg
```

Clang
```
sudo bash -c "$(wget -O - https://apt.llvm.org/llvm.sh)"
```

Other
```
sudo apt-get install -y portaudio19-dev build-essential libpulse-dev libdbus-1-dev pkg-config libx11-dev libatk1.0-dev libgtk-3-dev libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev libwayland-dev libxkbcommon-dev libopencv-dev
```

### Install _adder-viz_

To enable the use of _source-modeled lossy compression_ (the only such scheme for event-based video, as far as I'm
aware), install with the `compression` feature enabled:

```
cargo install adder-viz -F "compression"
```

To transcode from an iniVation DVS/DAVIS camera (using an older method, not yet unified with the Prophesee transcoder),
enable the `open-cv` feature:

```
cargo install adder-viz -F "compression open-cv"
```
