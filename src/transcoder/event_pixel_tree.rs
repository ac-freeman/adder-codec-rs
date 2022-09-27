use crate::transcoder::event_pixel::{Intensity, D};
use crate::{DeltaT, Event, EventCoordless, D_SHIFT};

struct PixelNode {
    // /// Will have the larger D value
    // main: Box<PixelNode>,
    /// Will have the smaller D value
    alt: Option<Box<PixelNode>>,

    d: D,
    integration: Intensity,
    delta_t: f32,
    best_event: Option<EventCoordless>,
}

impl PixelNode {
    pub fn new(start_intensity: Intensity, start_time: f32) -> PixelNode {
        let start_d = fast_math::log2_raw(start_intensity) as D;
        PixelNode {
            alt: None,
            d: start_d + 1,
            integration: start_intensity,
            delta_t: start_time,
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
        if self.integration + intensity >= D_SHIFT[self.d as usize] as f32 {
            let prop = (D_SHIFT[self.d as usize] as f32 - self.integration) as f32 / intensity;
            self.integration += intensity * prop;
            self.delta_t += time as f32 * prop;
            self.best_event = Some(EventCoordless {
                d: self.d,
                delta_t: (self.delta_t + time * prop) as DeltaT,
            });
            self.d += 1;
            self.alt = Some(Box::from(PixelNode::new(
                intensity - (intensity * prop),
                time - (time * prop),
            )));
        }
        self.integration += intensity;
        self.delta_t += time;
    }
}
