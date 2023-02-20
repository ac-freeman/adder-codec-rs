use adder_codec_core::{DeltaT, D, D_MAX};
use std::cmp::min;

#[allow(dead_code)]
#[derive(Copy, Clone)]
pub enum DecimationMode {
    Standard,
    AggressiveRoi,
    Manual,
}

#[derive(Default)]
pub(crate) struct DController {
    pub(crate) lookahead_d: D,
    // delta_t_max: DeltaT, // Should just pass this with each call?
    delta_t_predicted: DeltaT,
}

/// Define the shared functions of our `DController`
pub trait DControl {
    fn throttle_decimation(&mut self, d: &mut D, delta_t_max: DeltaT);
    fn update_decimation(&mut self, d: &mut D, delta_t: DeltaT, delta_t_max: DeltaT);
    // fn get_d(&self) -> &D;
    // fn get_d_mut(&mut self) -> &mut D;
    // fn set_d(&mut self, d: D);
    fn set_lookahead_d(&mut self, d: D);
    fn update_roi_factor(&mut self, roi_factor: u8);
}

/// Standard decimation mode only looks at intra-pixel Δt prediction accuracy when adjusting
/// [`D`]-values
#[derive(Default)]
pub(crate) struct Standard {
    pub(crate) controller: DController,
    unstable_bits: u8,
    delta_t_stable: DeltaT,
}
#[allow(dead_code)]
impl Standard {
    pub fn new() -> Standard {
        Standard {
            controller: DController {
                lookahead_d: u8::MAX,
                delta_t_predicted: 500,
            },
            unstable_bits: 0,
            delta_t_stable: 0,
        }
    }
}

impl DControl for Standard {
    fn throttle_decimation(&mut self, d: &mut D, delta_t_max: DeltaT) {
        self.unstable_bits = 32; // TODO: maybe make conditional
        throttle_decimation_general(&mut self.controller, d, delta_t_max);
    }

    /// Looks at Δt prediction accuracy and increases/decreases [`D`] accordingly
    fn update_decimation(&mut self, d: &mut D, delta_t: DeltaT, delta_t_max: DeltaT) {
        let last_unstable_bits = self.unstable_bits;

        let delta_t_predicted = &mut self.controller.delta_t_predicted;

        for i in (0..32).rev() {
            if (*delta_t_predicted >> i) & 1 != ((delta_t >> i) & 1) && self.unstable_bits == 0 {
                self.unstable_bits = i;
            }
        }
        if self.unstable_bits == 0 {
            self.delta_t_stable = min(self.delta_t_stable + delta_t, delta_t_max);
        } else {
            self.delta_t_stable = 0;
        }

        if last_unstable_bits > 0
            && self.unstable_bits < last_unstable_bits
            && *delta_t_predicted << 1 < delta_t_max
        {
            if *d < D_MAX && *delta_t_predicted << 1 <= delta_t_max / 2 {
                *d += 1;
                *delta_t_predicted <<= 1;
            } else {
                *delta_t_predicted = delta_t;
            }
        } else if self.unstable_bits > last_unstable_bits + 1 {
            if *d > 0 {
                *d -= 1;
            } else {
                *delta_t_predicted = delta_t;
            }
        } else if *d < D_MAX
            && (delta_t << 1 <= (delta_t_max * 2) / 3
                || (*delta_t_predicted << 1 < delta_t_max
                    && self.unstable_bits == 0
                    && self.delta_t_stable >= delta_t_max))
        {
            *delta_t_predicted = delta_t << 1;
            *d += 1;
        } else {
            *delta_t_predicted = delta_t;
        }
        // if *d > self.controller.lookahead_d && self.controller.lookahead_d != 255 {
        if self.controller.lookahead_d != 255 {
            *d = self.controller.lookahead_d;
        }
    }

    /// Getter method
    // fn get_d(&self) -> &D {
    //     &self.controller.d
    // }

    /// Getter method
    // fn get_d_mut(&mut self) -> &mut D {
    //     &mut self.controller.d
    // }

    // /// Setter method. Should only be called by high-level rate control functions which
    // /// override the default behavior
    // fn set_d(&mut self, d: D) {
    //     self.controller.d = d;
    // }

    fn set_lookahead_d(&mut self, d: D) {
        self.controller.lookahead_d = d;
    }

    /// Should not be called for Standard D-control. Results in error and panics.
    fn update_roi_factor(&mut self, _roi_factor: u8) {
        // panic!("Attempted to update roi_factor for standard D-control");
    }
}

/// Throttle down the [`D`] value, to hopefully ensure the next event won't be empty
fn throttle_decimation_general(controller: &mut DController, d: &mut D, delta_t_max: DeltaT) {
    let old_d = *d;
    let threshold = controller.delta_t_predicted as f32 * 1.2;
    if *d > 0 && delta_t_max > threshold as u32 {
        *d = fast_math::log2_raw(f32::from(*d)) as D;
        if *d > 0 && *d == old_d {
            *d -= 1;
        }
        controller.delta_t_predicted >>= old_d - *d;
    } else if *d > 0 {
        *d -= 1;
        controller.delta_t_predicted = delta_t_max >> 1;
    }
    if *d > controller.lookahead_d && controller.lookahead_d != 255 {
        *d = controller.lookahead_d;
    }
}

/// Aggressive decimation scheme tries to make every pixel have as high a [`D`]-value as possible,
/// without generating empty events. Also incorporates an [`roi_factor`](Aggressive::roi_factor), to preemptively lower [`D`]
/// where necessary
#[derive(Default)]
pub(crate) struct Aggressive {
    pub(crate) controller: DController,

    /// A higher `roi_factor` means the pixel is closer to an ROI. 0-value means the pixel is
    /// not in any ROI
    roi_factor: u8,
    ref_time: DeltaT,
}

#[allow(dead_code)]
impl Aggressive {
    pub fn new(ref_time: DeltaT, _delta_t_max: DeltaT) -> Aggressive {
        Aggressive {
            controller: DController {
                lookahead_d: u8::MAX,
                delta_t_predicted: 500,
            },
            roi_factor: 1_u8,
            ref_time,
        }
    }
}

impl DControl for Aggressive {
    fn throttle_decimation(&mut self, d: &mut D, delta_t_max: DeltaT) {
        throttle_decimation_general(&mut self.controller, d, delta_t_max);
    }

    /// Adjust [`D`] based on current Δt and proximity to ROI
    fn update_decimation(&mut self, d: &mut D, delta_t: DeltaT, delta_t_max: DeltaT) {
        if self.roi_factor == (delta_t_max / self.ref_time) as u8 {
            if *d < D_MAX && delta_t << 1 <= self.ref_time {
                *d += 1;
            } else if *d > 0 && delta_t > self.ref_time {
                *d = adder_codec_core::D_START;
            }
        } else if *d < D_MAX
            && (delta_t << 1) as f32 <= (delta_t_max / u32::from(self.roi_factor)) as f32 * 0.8
        {
            *d += 1;
        } else if *d > 0 && delta_t > delta_t_max / u32::from(self.roi_factor) {
            *d -= 1;
        }
        if *d > self.controller.lookahead_d && self.controller.lookahead_d != 255 {
            *d = self.controller.lookahead_d;
        }
    }

    // /// Getter method
    // fn get_d(&self) -> &D {
    //     &self.controller.d
    // }
    //
    // /// Getter method
    // fn get_d_mut(&mut self) -> &mut D {
    //     &mut self.controller.d
    // }
    //
    // /// Setter method. Should only be called by functions which override default behavior
    // fn set_d(&mut self, d: D) {
    //     self.controller.d = d;
    // }

    fn set_lookahead_d(&mut self, d: D) {
        self.controller.lookahead_d = d;
    }

    /// Reset [`roi_factor`](Aggressive::roi_factor) if this pixel is in an ROI, or else slowly lower the [`roi_factor`](Aggressive::roi_factor)
    /// if the pixel was recently in an ROI
    fn update_roi_factor(&mut self, roi_factor: u8) {
        if roi_factor > 1 {
            self.roi_factor = roi_factor;
        } else if self.roi_factor > 1 {
            // Gradually lower pixel sensitivity if it was recently close to or in an ROI
            self.roi_factor -= 1;
        }
    }
}

/// Manual decimation mode is set entirely by higher level controller. This is effectively a placeholder
#[derive(Default)]
pub(crate) struct Manual {}

#[allow(dead_code)]
impl Manual {
    pub fn new() -> Manual {
        Manual {}
    }
}

impl DControl for Manual {
    fn throttle_decimation(&mut self, _d: &mut D, _delta_t_max: DeltaT) {
        todo!()
    }

    fn update_decimation(&mut self, _d: &mut D, _delta_t: DeltaT, _delta_t_max: DeltaT) {
        todo!()
    }

    fn set_lookahead_d(&mut self, _d: D) {
        todo!()
    }

    fn update_roi_factor(&mut self, _roi_factor: u8) {
        todo!()
    }
}

unsafe impl Sync for DController {}
unsafe impl Send for DController {}
