/// Decimation value; a pixel's sensitivity.
pub type D = u8;

type Integration = f32;

/// Number of ticks elapsed since a given pixel last fired an [`pixel::Event`]
pub type DeltaT = u32;

/// Measure of an amount of light intensity
pub type Intensity = f32;

/// Pixel x- or y- coordinate address in the ADΔER model
pub type PixelAddress = u16;

pub mod pixel {
    use crate::transcoder::d_controller::{Aggressive, DControl, DecimationModes, Standard};
    use crate::transcoder::event_pixel::{DeltaT, Integration, Intensity, PixelAddress, D};
    use crate::{Coord, Event, D_SHIFT, MAX_INTENSITY};

    /// The last [`Event`] fired by a pixel
    #[derive(Debug, Copy, Clone, Default)]
    pub struct LastEvent {
        pub(crate) event: Event,
        frame_intensity: Intensity,
        frame_delta_t: Intensity,
    }

    #[derive(Copy, Clone)]
    pub struct Transition {
        pub(crate) frame_idx: u32,
    }

    /// ADΔER pixel model, with attributes for driving integration
    pub struct EventPixel {
        /// Pixel's current accumulated intensity
        integration: Integration,

        /// Pixel's current Δt (number of ticks elapsed since last event fired)
        delta_t: f32,

        /// The number of ticks left in the intensity to add to pixel's integration
        delta_t_original: f32,

        /// Number of ticks from the input intensity that will be added to the pixel's Δt.
        /// This is a subset of the input intensity's Δt.
        delta_t_to_add: f32,

        /// The proportion of `delta_t_to_add`/`delta_t_original`
        delta_t_portion: f32,

        /// The number of events the pixel has fired over the current reference interval
        pub(crate) fire_count: u8,

        /// The starting input intensity
        intensity_original: Integration,

        /// The event fired most recently by the pixel
        pub(crate) last_event: LastEvent,

        /// The pixel's scheme for adjusting its [`D`] value
        pub(crate) d_controller: Box<dyn DControl>,

        event_to_send: Event,

        pub(crate) next_transition: Transition,

        ref_time: DeltaT,
    }

    impl LastEvent {
        /// Calculate the instantaneous frame-length normalized intensity of the event
        pub fn calc_frame_intensity(&mut self, ref_time: u32) {
            self.frame_intensity = match self.event.delta_t {
                0 => 0.0,
                _ => {
                    ((1 << self.event.d as u32) as f32 / MAX_INTENSITY)
                        * (ref_time as f32 / (self.event.delta_t as f32))
                }
            };
        }

        /// Getter method
        pub fn _get_d(&self) -> D {
            self.event.d
        }

        pub fn calc_frame_delta_t(&mut self, delta_t_max: DeltaT) {
            self.frame_delta_t = self.event.delta_t as f32 / delta_t_max as f32;
        }

        /// Getter method. Should only be called after calc_frame_intensity
        pub fn get_frame_intensity(&self) -> Intensity {
            self.frame_intensity
        }

        /// Getter method. Should only be called after calc_frame_delta_t
        pub fn get_frame_delta_t(&self) -> Intensity {
            self.frame_delta_t
        }
    }

    impl EventPixel {
        /// Initialize pixel
        pub fn new(
            y: PixelAddress,
            x: PixelAddress,
            c: u8,
            ref_time: DeltaT,
            delta_t_max: DeltaT,
            d_mode: u32,
            channels: u8,
        ) -> EventPixel {
            let d_controller: Box<dyn DControl> = match d_mode {
                d_mode if d_mode == DecimationModes::Standard as u32 => Box::new(Standard::new()),
                d_mode if d_mode == DecimationModes::AggressiveRoi as u32 => {
                    Box::new(Aggressive::new(ref_time, delta_t_max))
                }
                _ => panic!("Invalid d_mode"),
            };

            EventPixel {
                integration: 0.0,
                delta_t: 0.0,
                delta_t_original: 0.0,
                delta_t_to_add: 0.0,
                delta_t_portion: 0.0,
                fire_count: 0,
                intensity_original: 0.0,
                last_event: LastEvent {
                    event: Event {
                        coord: Coord {
                            x,
                            y,
                            c: match channels {
                                1 => None,
                                _ => Some(c),
                            },
                        },
                        d: 0,
                        delta_t: 0,
                    },
                    frame_intensity: 0.0,
                    frame_delta_t: 0.0,
                },
                d_controller,
                event_to_send: Event {
                    coord: Coord {
                        x,
                        y,
                        c: match channels {
                            1 => None,
                            _ => Some(c),
                        },
                    },
                    d: 0,
                    delta_t: 0,
                },
                next_transition: Transition { frame_idx: 1 },
                ref_time,
            }
        }

        /// Reset the count of the number of events fired by the pixel over a given period of time
        pub fn reset_fire_count(&mut self) {
            self.fire_count = 0;
        }

        /// Add the given [`Intensity`] value to the pixel's integration, and fire events as
        /// necessary
        pub fn add_intensity(
            &mut self,
            mut intensity_left: Intensity,
            mut delta_t_left: f32,
            delta_t_max: &DeltaT,
            sender: &mut Vec<Event>,
            communicate_events: bool,
        ) {
            assert!(delta_t_left > 0.0);

            self.intensity_original = intensity_left;
            self.delta_t_original = delta_t_left;
            let mut first_iter = true;

            loop {
                let x: bool = match (
                    intensity_left,
                    (*delta_t_max as f32),
                    self.intensity_original,
                    delta_t_left,
                ) {
                    (_, b, _, _) if self.has_empty_event(&b) => {
                        self.fire_event(
                            true,
                            delta_t_max,
                            &b,
                            sender,
                            communicate_events,
                            first_iter,
                        );
                        true
                    }
                    (a, b, _, _) if self.has_full_event(&a) => {
                        self.delta_t_to_add = ((D_SHIFT[*self.d_controller.get_d() as usize]
                            as f32
                            - self.integration)
                            / self.intensity_original)
                            * self.delta_t_original as f32;
                        if self.delta_t + self.delta_t_to_add > b {
                            self.consume_to_delta_t_max(&b, &mut intensity_left, &mut delta_t_left);
                            continue;
                        }
                        self.delta_t += self.delta_t_to_add;
                        delta_t_left -= self.delta_t_to_add;
                        intensity_left -=
                            D_SHIFT[*self.d_controller.get_d() as usize] as f32 - self.integration;
                        self.fire_event(
                            false,
                            delta_t_max,
                            &b,
                            sender,
                            communicate_events,
                            first_iter,
                        );

                        true
                    }
                    (a, b, _, d) if (self.intensity_original == 0.0 && d > 0.0) || a > 0.0 => {
                        self.delta_t_to_add = delta_t_left;
                        if self.delta_t + self.delta_t_to_add > b {
                            self.consume_to_delta_t_max(&b, &mut intensity_left, &mut delta_t_left);

                            continue;
                        }
                        self.delta_t += self.delta_t_to_add;
                        self.integration += intensity_left;

                        // For testing
                        delta_t_left -= self.delta_t_to_add;
                        intensity_left = 0.0;
                        true
                    }
                    (_, _, _, _) => false,
                };

                // If we didn't match any of the test cases, then there's nothing left to integrate.
                // Break out of the loop.
                if !x {
                    break;
                }
                first_iter = false;
            }

            assert_eq!(delta_t_left as u32, 0);

            // Allow some slack for floating point errors
            if intensity_left.abs() >= 2.0e-3_f32 {
                eprintln!("ERROR: Intensity left: {}", intensity_left);
            }
            assert!(intensity_left.abs() < 2.0e-3_f32);
        }

        /// Integrate intensity to the point that delta_t_max is reached
        fn consume_to_delta_t_max(
            &mut self,
            delta_t_max_f32: &f32,
            intensity_left: &mut Intensity,
            delta_t_left: &mut f32,
        ) {
            self.delta_t_to_add = *delta_t_max_f32 - self.delta_t;
            self.delta_t = *delta_t_max_f32;
            self.delta_t_portion = self.delta_t_to_add as f32 / self.delta_t_original as f32;
            *intensity_left -= self.delta_t_portion * self.intensity_original;
            *delta_t_left -= self.delta_t_to_add;
        }

        /// Returns `true` if the pixel has met the conditions for an [`Event`] of either type,
        /// `false` if not
        fn _has_event(&mut self, intensity_left: &Intensity, delta_t_max_f32: &f32) -> bool {
            self.has_empty_event(delta_t_max_f32) || self.has_full_event(intensity_left)
        }

        /// Returns `true` if the pixel has met the conditions for an [`Event`] by means of
        /// reaching the intensity integration threshold, `false` if not
        fn has_full_event(&mut self, intensity_left: &Intensity) -> bool {
            self.integration + *intensity_left
                >= D_SHIFT[*self.d_controller.get_d() as usize] as f32
        }

        /// Returns `true` if the pixel has met the conditions for an [`Event`] by means of
        /// reaching the delta_t_max temporal threshold, `false` if not
        fn has_empty_event(&mut self, delta_t_max_f32: &f32) -> bool {
            self.delta_t >= *delta_t_max_f32
        }

        /// Form the event and send it as a message to the
        /// [`OutputWriter`](crate::processor::output_writer::OutputWriter) channel, then reset
        /// pixel integration state
        fn fire_event(
            &mut self,
            empty: bool,
            delta_t_max: &DeltaT,
            delta_t_max_f32: &f32,
            sender: &mut Vec<Event>,
            communicate_events: bool,
            first_iter: bool,
        ) {
            if empty {
                assert_eq!(self.delta_t, *delta_t_max_f32);
                self.event_to_send.d = 0;
                self.event_to_send.delta_t = *delta_t_max;
                // self.last_event.event = self.event_to_send;  // TODO: remove this again?

                if communicate_events {
                    sender.push(self.event_to_send);
                }

                // self.d_controller.throttle_decimation(*delta_t_max);
            } else {
                self.event_to_send.d = *self.d_controller.get_d();
                self.event_to_send.delta_t = self.delta_t as u32;

                assert!(self.delta_t > 0.0);
                debug_assert!(self.delta_t <= *delta_t_max_f32);

                // self.d_controller
                //     .update_decimation(self.delta_t as u32, *delta_t_max);

                // last_event is used for calculating the instantaneous intensities
                if first_iter {
                    self.last_event.event = self.event_to_send;
                    if *self.d_controller.get_d() > D_SHIFT.len() as D {
                        self.last_event.event.d = 0;
                    }
                }

                if communicate_events {
                    sender.push(self.event_to_send);
                }
            }

            self.fire_count = self.fire_count.saturating_add(1); // TODO: handle error if overflow encountered
                                                                 // if self.fire_count > 2 {
                                                                 //     print!("a");
                                                                 // }
            self.integration = 0.0;
            self.delta_t = 0.0;
        }

        pub fn lookahead_reset(&mut self, sender: &mut Vec<Event>) {
            assert!(self.integration < 255.0);
            if self.delta_t > 0.0 {
                self.d_controller.set_d(255);
                if self.integration == 0.0 {
                    self.d_controller.set_d(254);
                    if self.delta_t >= self.ref_time as f32 {
                        self.fire_event(false, &0, &f32::MAX, sender, true, true);
                    }
                } else {
                    assert!(self.delta_t <= self.ref_time as f32);
                    self.delta_t = (self.ref_time
                        - (self.last_event.event.delta_t as DeltaT % self.ref_time))
                        as f32;
                    assert_eq!(
                        (self.last_event.event.delta_t % self.ref_time) + self.delta_t as DeltaT,
                        self.ref_time
                    );
                }
                self.integration = 0.0;
                self.delta_t = 0.0;
            }
            assert_eq!(self.integration, 0.0);
        }
    }
    unsafe impl Sync for EventPixel {}
    unsafe impl Send for EventPixel {}
}
