/// Tools for transcoding from a DVS/DAVIS video source to ADΔER
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
/*0*/    [0.0,     0.0,         20.0,                10.0,                     1.0/20.0],
/*1*/    [0.0,     2.0,         25.0,                 9.0,                     1.0/22.0],
/*2*/    [1.0,     2.0,         30.0,                 8.0,                     1.0/26.0],
/*3*/    [2.0,     4.0,         35.0,                 7.0,                     1.0/28.0],
/*4*/    [3.0,    5.0,         40.0,                 6.0,                     1.0/30.0],
/*5*/    [4.0,    10.0,         45.0,                 5.0,                     1.0/32.0],
/*6*/    [5.0,    15.0,         50.0,                 4.0,                     1.0/34.0],
/*7*/    [6.0,    20.0,         55.0,                 3.0,                     1.0/36.0],
/*8*/    [7.0,    30.0,         60.0,                 2.0,                     1.0/38.0],
/*9*/    [8.0,   40.0,         65.0,                 1.0,                     1.0/40.0],
];

/// The default CRF quality level
pub const DEFAULT_CRF_QUALITY: u8 = 5;
