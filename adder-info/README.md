[![Crates.io](https://img.shields.io/crates/v/adder-info)](https://crates.io/crates/adder-info)
[![Downloads](https://img.shields.io/crates/dr/adder-info)](https://crates.io/crates/adder-info)

## Inspect an ADΔER file

Want to quickly view the metadata for an ADΔER file? Just install this program for the current user with `cargo install adder-info`, then run with `adder-info -- -i /path/to/file.adder -d`. This program is analogous to `ffprobe` for framed video.

The `-d` flag enables the calculation of the ADΔER file's dynamic range. This can take a while, since each event must be decoded to find the event with the maximum intensity and the minimum intensity. Example output:

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
