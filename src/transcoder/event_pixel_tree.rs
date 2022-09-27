use crate::transcoder::event_pixel::{Intensity, D};
use crate::{DeltaT, Event, EventCoordless, D_SHIFT};
use std::mem;

#[derive(Copy, Clone)]
struct PixelState {
    d: D,
    integration: Intensity,
    delta_t: f32,
}

struct PixelNode {
    /// Will have the smaller D value
    alt: Option<Box<PixelNode>>,

    state: PixelState,
    best_event: Option<EventCoordless>,
}

impl PixelNode {
    pub fn new(start_intensity: Intensity, start_time: f32) -> PixelNode {
        let start_d = fast_math::log2_raw(start_intensity) as D;
        PixelNode {
            alt: None,
            state: PixelState {
                d: start_d + 1,
                integration: start_intensity,
                delta_t: start_time,
            },
            best_event: None,
        }
    }

    pub fn integrate(&mut self, intensity: Intensity, time: f32) {
        self.integrate_main(intensity, time);

        if self.alt.is_some() {
            self.alt.as_mut().unwrap().integrate_main(intensity, time)
        }
    }

    pub fn integrate_main(&mut self, intensity: Intensity, time: f32) {
        if self.state.integration + intensity >= D_SHIFT[self.state.d as usize] as f32 {
            let prop =
                (D_SHIFT[self.state.d as usize] as f32 - self.state.integration) as f32 / intensity;
            self.state.integration += intensity * prop;
            self.state.delta_t += time as f32 * prop;
            self.best_event = Some(EventCoordless {
                d: self.state.d,
                delta_t: (self.state.delta_t + time * prop) as DeltaT,
            });
            self.state.d += 1;

            // If there was previously an alt node, it's automatically dropped when it leaves scope
            self.alt = Some(Box::from(PixelNode::new(
                intensity - (intensity * prop),
                time - (time * prop),
            )));
        }
        self.state.integration += intensity;
        self.state.delta_t += time;
    }

    /// Recursively pop all the alt events
    pub fn pop_best_events(&mut self) -> Vec<EventCoordless> {
        let res = self.pop_and_reset_state();
        self.state = res.1;
        self.alt = None; // Free the memory for the alternate branch
        self.best_event = None;
        res.0
    }

    fn pop_and_reset_state(&mut self) -> (Vec<EventCoordless>, PixelState) {
        match self.best_event {
            None => {
                panic!("No best event! TODO: handle it")
            }
            Some(event) => {
                let mut ret = vec![event];

                let res = match self.alt.is_some() {
                    false => (vec![], self.state.clone()),
                    true => self.alt.as_mut().unwrap().pop_and_reset_state(),
                };
                ret.extend(res.0);
                (ret, res.1)
            }
        }
    }
}
