# ADDER-codec-rs
[![Build Status](https://github.com/ac-freeman/adder-codec-rs/workflows/Rust/badge.svg)](https://github.com/ac-freeman/adder-codec-rs/actions)
[![Documentation](https://docs.rs/adder-codec-rs/badge.svg)](https://docs.rs/adder-codec-rs)
[![codecov](https://codecov.io/gh/ac-freeman/adder-codec-rs/branch/main/graph/badge.svg?token=P0MSB1CJSE)](https://codecov.io/gh/ac-freeman/adder-codec-rs)
[![Crates.io](https://img.shields.io/crates/v/adder-codec-rs)](https://crates.io/crates/adder-codec-rs)
[![Downloads](https://img.shields.io/crates/dr/adder-codec-rs)](https://crates.io/crates/adder-codec-rs)


A unified framework for event-based video. Encoder/transcoder/decoder for ADΔER (Address, Decimation, Δt Event Representation) video streams. Includes a transcoder for casting framed video into an ADΔER representation in a manner which preserves the temporal synchronicity of the source, but enables many-frame intensity averaging on a per-pixel basis and extremely high dynamic range.

![blender_output2_30_mid](https://github.com/ac-freeman/adder-codec-rs/assets/19912588/4d1d9fc2-6a9d-49ab-b4da-07c2bb88a839)


To enable the use of _source-modeled lossy compression_ (the only such scheme for event-based video, as far as I'm aware), install/import the relevant crates below with the `compression` feature enabled and the _nightly_ toolchain. For example, install adder-viz as follows:
```
cargo +nightly install adder-viz -F "compression open-cv"
```

Source 8-bit image frame with shadows boosted ([source video](https://www.pexels.com/video/river-between-trees-2126081/))      |  Frame reconstructed from ADΔER events, generated from 48 input frames, with shadows boosted. Note the greater dynamic range and temporal denoising in the shadows.
:-------------------------:|:-------------------------:
![](adder-codec-rs/source_frame_0.jpg)  |  ![](adder-codec-rs/out_16bit_2_c10.jpg)

## Included crates
- **adder-codec-rs**: ADΔER transcoders [[source](adder-codec-rs)] [![Crates.io](https://img.shields.io/crates/v/adder-codec-rs)](https://crates.io/crates/adder-codec-rs)
- **adder-codec-core**: core library [[source](adder-codec-core)] [![Crates.io](https://img.shields.io/crates/v/adder-codec-core)](https://crates.io/crates/adder-codec-core)
- **adder-info**: tool for reading metadata of a .adder file [[source](adder-info)] [![Crates.io](https://img.shields.io/crates/v/adder-info)](https://crates.io/crates/adder-info)
- **adder-to-dvs**: tool for quickly converting a .adder file to a reasonable DVS representation in a text format [[source](adder-to-dvs)] [![Crates.io](https://img.shields.io/crates/v/adder-to-dvs)](https://crates.io/crates/adder-to-dvs)
- **adder-viz**: GUI application for transcoding framed and event (DVS/DAVIS) video to ADΔER, playing back .adder files, and visualizing the _many_ available ADΔER parameters [[source](adder-viz)] [![Crates.io](https://img.shields.io/crates/v/adder-viz)](https://crates.io/crates/adder-viz)

## Other crates
- **davis-edi-rs**: a super high-speed system for performing frame deblurring and framed reconstruction from DAVIS/DVS streams, forming the backbone for the event camera driver code in the ADΔER library [[source](https://github.com/ac-freeman/davis-EDI-rs)] [![Crates.io](https://img.shields.io/crates/v/davis-edi-rs)](https://crates.io/crates/davis-edi-rs)
- **aedat-rs**: a fast AEDAT 4 Rust reader. [[source](https://github.com/ac-freeman/aedat-rs)] [![Crates.io](https://img.shields.io/crates/v/aedat)](https://crates.io/crates/aedat)

## More information
Read the [wiki](https://github.com/ac-freeman/adder-codec-rs/wiki/) for background on ADΔER, how to use it, and what tools are included.

## Cite this work

If you write a paper which references this software, we ask that you reference the following papers on which it is based. Citations are given in the BibTeX format.

[An Asynchronous Intensity Representation for Framed and Event Video Sources](https://arxiv.org/abs/2301.08783)

**Note:** The code associated with this paper was released in [version 0.2.0](https://github.com/ac-freeman/adder-codec-rs/releases/tag/v0.2.0)
```bibtex
@inproceedings{10.1145/3587819.3590969,
author = {Freeman, Andrew C. and Singh, Montek and Mayer-Patel, Ketan},
title = {An Asynchronous Intensity Representation for Framed and Event Video Sources},
year = {2023},
isbn = {979-8-4007-0148-1/23/06},
publisher = {Association for Computing Machinery},
address = {New York, NY, USA},
url = {https://doi.org/10.1145/3587819.3590969},
doi = {10.1145/3587819.3590969},
booktitle = {Proceedings of the 14th ACM Multimedia Systems Conference},
pages = {1–12},
numpages = {12},
location = {Vancouver, BC, Canada},
series = {MMSys '23}
}
```

[The ADΔER Framework: Tools for Event Video Representations](https://dl.acm.org/doi/pdf/10.1145/3587819.3593028)
```bibtex
@inproceedings{Freeman23-0,
  title = {The ADΔER Framework: Tools for Event Video Representations},
  author = {Andrew C. Freeman},
  year = {2023},
  doi = {10.1145/3587819.3593028},
  url = {https://doi.org/10.1145/3587819.3593028},
  researchr = {https://researchr.org/publication/Freeman23-0},
  cites = {0},
  citedby = {0},
  pages = {343-347},
  booktitle = {Proceedings of the 14th Conference on ACM Multimedia Systems, MMSys 2023, Vancouver, BC, Canada, June 7-10, 2023},
  publisher = {ACM},
}
```

[Motion segmentation and tracking for integrating event cameras](https://dl.acm.org/doi/abs/10.1145/3458305.3463373)
```bibtex
@inproceedings{10.1145/3458305.3463373,
author = {Freeman, Andrew C. and Burgess, Chris and Mayer-Patel, Ketan},
title = {Motion Segmentation and Tracking for Integrating Event Cameras},
year = {2021},
isbn = {9781450384346},
publisher = {Association for Computing Machinery},
address = {New York, NY, USA},
url = {https://doi.org/10.1145/3458305.3463373},
doi = {10.1145/3458305.3463373},
abstract = {Integrating event cameras are asynchronous sensors wherein incident light values may be measured directly through continuous integration, with individual pixels' light sensitivity being adjustable in real time, allowing for extremely high frame rate and high dynamic range video capture. This paper builds on lessons learned with previous attempts to compress event data and presents a new scheme for event compression that has many analogues to traditional framed video compression techniques. We show how traditional video can be transcoded to an event-based representation, and describe the direct encoding of motion data in our event-based representation. Finally, we present experimental results proving how our simple scheme already approaches the state-of-the-art compression performance for slow-motion object tracking. This system introduces an application "in the loop" framework, where the application dynamically informs the camera how sensitive each pixel should be, based on the efficacy of the most recent data received.},
booktitle = {Proceedings of the 12th ACM Multimedia Systems Conference},
pages = {1–11},
numpages = {11},
keywords = {HDR, spike compression, image reconstruction, simulation, event cameras, object tracking, entropy encoding, motion segmentation, asynchronous systems},
location = {Istanbul, Turkey},
series = {MMSys '21}
}
```

[Integrating Event Camera Sensor Emulator](https://dl.acm.org/doi/10.1145/3394171.3414394)
```bibtex
@inproceedings{10.1145/3394171.3414394,
author = {Freeman, Andrew C. and Mayer-Patel, Ketan},
title = {Integrating Event Camera Sensor Emulator},
year = {2020},
isbn = {9781450379885},
publisher = {Association for Computing Machinery},
address = {New York, NY, USA},
url = {https://doi.org/10.1145/3394171.3414394},
doi = {10.1145/3394171.3414394},
abstract = {Event cameras are biologically-inspired sensors that upend the framed, synchronous nature of traditional cameras. Singh et al. proposed a novel sensor design wherein incident light values may be measured directly through continuous integration, with individual pixels' light sensitivity being adjustable in real time, allowing for extremely high frame rate and high dynamic range video capture. Arguing the potential usefulness of this sensor, this paper introduces a system for simulating the sensor's event outputs and pixel firing rate control from 3D-rendered input images.},
booktitle = {Proceedings of the 28th ACM International Conference on Multimedia},
pages = {4503–4505},
numpages = {3},
keywords = {asynchronous systems, image reconstruction, spike compression, event cameras, HDR, simulation},
location = {Seattle, WA, USA},
series = {MM '20}
}
```
