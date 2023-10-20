/// Tools for transcoding from a DVS/DAVIS video source to ADΔER
#[cfg(feature = "open-cv")]
pub mod davis;

/// Tools for transcoding from a framed video source to ADΔER
pub mod framed;

/// Common functions and structs for all transcoder sources
pub mod video;

/// Constant Rate Factor lookup table
#[rustfmt::skip]
pub static CRF: [[f32; 5]; 10] = [ 
// baseline C     max C    Dt_max mutliplier    C increase velocity             feature radius
//                           (X*dt_ref)    (+1 C every X*dt_ref time)   (X * min resolution, in pixels)
/*0*/    [0.0,     0.0,         40.0,                10.0,                     1E-9],
/*1*/    [0.0,     3.0,         80.0,                 9.0,                     1.0/12.0],
/*2*/    [1.0,     5.0,         120.0,                 8.0,                     1.0/14.0],
/*3*/    [2.0,     7.0,         160.0,                 7.0,                     1.0/15.0],
/*4*/    [3.0,    9.0,         200.0,                 6.0,                     1.0/16.0],
/*5*/    [3.0,    10.0,         240.0,                 5.0,                     1.0/17.0],
/*6*/    [4.0,    15.0,         280.0,                 4.0,                     1.0/18.0],
/*7*/    [5.0,    20.0,         320.0,                 3.0,                     1.0/20.0],
/*8*/    [6.0,    30.0,         360.0,                 2.0,                     1.0/22.0],
/*9*/    [7.0,   40.0,         400.0,                 1.0,                     1.0/25.0],
];

/// The default CRF quality level
pub const DEFAULT_CRF_QUALITY: u8 = 3;
