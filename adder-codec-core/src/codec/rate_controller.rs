use crate::PlaneSize;

/// Constant Rate Factor lookup table
#[rustfmt::skip]
pub static CRF: [[f32; 4]; 10] = [
// baseline C     max C                 C increase velocity             feature radius
//                                      (+1 C every X*dt_ref time)   (X * min resolution, in pixels)
    /*0*/    [0.0,     0.0,                     10.0,                     1E-9],
    /*1*/    [0.0,     1.0,                      9.0,                     1.0/12.0],
    /*2*/    [1.0,     3.0,                       8.0,                     1.0/14.0],
    /*3*/    [2.0,     7.0,                       7.0,                     1.0/15.0],
    /*4*/    [5.0,    9.0,                      6.0,                     1.0/18.0],
    /*5*/    [6.0,    10.0,                       5.0,                     1.0/20.0],
    /*6*/    [7.0,    13.0,                       4.0,                     1.0/25.0],
    /*7*/    [8.0,    16.0,                       3.0,                     1.0/30.0],
    /*8*/    [10.0,    20.0,                       2.0,                     1.0/32.0],
    /*9*/    [15.0,   25.0,                      1.0,                     1.0/35.0],
];

/// The default CRF quality level
pub const DEFAULT_CRF_QUALITY: u8 = 3;

#[derive(Copy, Clone, PartialEq, Debug)]
pub struct Crf {
    /// Constant Rate Factor (CRF) quality setting for the encoder. 0 is lossless, 9 is worst quality.
    /// Determines:
    /// * The baseline (starting) c-threshold for all pixels
    /// * The maximum c-threshold for all pixels
    /// * The Dt_max multiplier
    /// * The c-threshold increase velocity (how often to increase C if the intensity is stable)
    /// * The radius for which to reset the c-threshold for neighboring pixels (if feature detection is enabled)
    crf_quality: Option<u8>,

    parameters: CrfParameters,
}

#[derive(Copy, Clone, PartialEq, Debug)]
pub struct CrfParameters {
    /// The baseline (starting) contrast threshold for all pixels
    pub c_thresh_baseline: u8,

    /// The maximum contrast threshold for all pixels
    pub c_thresh_max: u8,

    /// The velocity at which to increase the contrast threshold for all pixels (increment c by 1
    /// for every X input intervals, if it's stable)
    pub c_increase_velocity: u8,

    /// The radius for which to reset the c-threshold for neighboring pixels (if feature detection is enabled)
    pub feature_c_radius: u16,
}

impl Crf {
    pub fn new(crf: Option<u8>, plane: PlaneSize) -> Self {
        let default_crf = crf.unwrap_or(DEFAULT_CRF_QUALITY);

        Crf {
            crf_quality: crf,
            parameters: CrfParameters {
                c_thresh_baseline: CRF[default_crf as usize][0] as u8,
                c_thresh_max: CRF[default_crf as usize][1] as u8,
                c_increase_velocity: CRF[default_crf as usize][2] as u8,
                feature_c_radius: (CRF[default_crf as usize][3] * plane.min_resolution() as f32)
                    as u16,
            },
        }
    }

    pub fn override_c_thresh_baseline(&mut self, baseline: u8) {
        self.parameters.c_thresh_baseline = baseline;
        self.crf_quality = None;
    }

    pub fn override_c_thresh_max(&mut self, max: u8) {
        self.parameters.c_thresh_max = max;
        self.crf_quality = None;
    }

    pub fn override_c_increase_velocity(&mut self, velocity: u8) {
        self.parameters.c_increase_velocity = velocity;
        self.crf_quality = None;
    }

    pub fn override_feature_c_radius(&mut self, radius: u16) {
        self.parameters.feature_c_radius = radius;
        self.crf_quality = None;
    }

    pub fn get_parameters(&self) -> &CrfParameters {
        &self.parameters
    }

    pub fn get_parameters_mut(&mut self) -> &mut CrfParameters {
        &mut self.parameters
    }

    pub fn get_quality(&self) -> Option<u8> {
        self.crf_quality
    }
}
