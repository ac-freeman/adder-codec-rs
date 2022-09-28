use crate::transcoder::event_pixel::{Intensity, D};
use crate::transcoder::event_pixel_tree::Mode::{Continuous, FramePerfect};
use crate::{DeltaT, Event, EventCoordless, SourceCamera, D_MAX, D_SHIFT};
use std::mem;

pub(crate) enum Mode {
    FramePerfect,
    Continuous,
}

#[derive(Copy, Clone)]
struct PixelState {
    d: D,
    integration: Intensity,
    delta_t: f32,
}

pub struct PixelNode {
    /// Will have the smaller D value
    alt: Option<Box<PixelNode>>,

    state: PixelState,
    best_event: Option<EventCoordless>,
}

impl PixelNode {
    pub fn new(start_intensity: Intensity) -> PixelNode {
        let start_d = fast_math::log2_raw(start_intensity) as D;
        assert!(start_d <= D_MAX);
        PixelNode {
            alt: None,
            state: PixelState {
                d: start_d,
                integration: 0.0,
                delta_t: 0.0,
            },
            best_event: None,
        }
    }

    pub fn set_d(&mut self, d: D) {
        self.state.d = d;
    }

    // Integrates the intensity. Returns bool indicating whether or not the events MUST be popped
    // or else risk losing accuracy. Should only return true when d=D_MAX, which should be
    // extremely rare
    pub fn integrate(
        &mut self,
        intensity: Intensity,
        time: f32,
        mode: &Mode,
        dtm: &DeltaT,
    ) -> bool {
        // debug_assert!(intensity <= 255.0);
        // debug_assert_ne!(intensity, 0.0);
        // debug_assert_ne!(time, 0.0);
        // assert_ne!(self.state.d, D_MAX);
        match self.integrate_main(intensity, time, mode) {
            None => {
                // Only should do when the main has not just fired and created the alt
                if self.alt.is_some() {
                    self.alt
                        .as_mut()
                        .unwrap()
                        .integrate(intensity, time, mode, dtm);
                }
            }
            Some((alt, intensity, time)) => {
                self.alt = Some(alt);
                self.alt
                    .as_mut()
                    .unwrap()
                    .integrate(intensity, time, mode, dtm);
            }
        }
        debug_assert!(D_SHIFT[self.state.d as usize] as Intensity > self.state.integration);
        return self.state.d == D_MAX || self.state.delta_t as DeltaT >= *dtm;
        // && self
        //     .best_event
        //     .unwrap_or(EventCoordless { d: 0, delta_t: 0 })
        //     .d
        //     == D_MAX;
    }

    pub fn integrate_main(
        &mut self,
        intensity: Intensity,
        time: f32,
        mode: &Mode,
    ) -> Option<(Box<PixelNode>, Intensity, f32)> {
        return if self.state.integration + intensity >= D_SHIFT[self.state.d as usize] as f32 {
            let prop =
                (D_SHIFT[self.state.d as usize] as f32 - self.state.integration) as f32 / intensity;
            assert!(prop > 0.0);
            self.best_event = Some(EventCoordless {
                d: self.state.d,
                delta_t: (self.state.delta_t + time * prop) as DeltaT,
            });
            if self.state.d < D_MAX {
                self.state.integration += intensity;
                self.state.delta_t += time;
                loop {
                    self.state.d += 1;
                    if D_SHIFT[self.state.d as usize] > self.state.integration as u32 {
                        break;
                    }
                }
            } else {
                dbg!(self.state.integration);
            }

            if intensity - (intensity * prop) >= 0.0 {
                // For a framed source, we need to return 0,0 for intensity,time.
                // This lets us preserve the spatially-coherent intensities, especially for color
                // transcode.
                return Some(match mode {
                    FramePerfect => (Box::from(PixelNode::new(intensity)), 0.0, 0.0),
                    Continuous => (
                        Box::from(PixelNode::new(intensity)),
                        intensity - (intensity * prop),
                        time - (time * prop),
                    ),
                });
            }
            None
        } else {
            self.state.integration += intensity;
            self.state.delta_t += time;
            None
        };
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
                // panic!("No best event! TODO: handle it")
                (vec![], self.state.clone())
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

#[cfg(test)]
mod tests {
    // Note this useful idiom: importing names from outer (for mod tests) scope.
    use super::*;

    fn make_tree() -> PixelNode {
        let dtm = 10000;
        let mut tree = PixelNode::new(100.0);
        assert_eq!(tree.state.d, 6);
        tree.integrate(100.0, 20.0, &Continuous, &dtm);
        assert!(tree.best_event.is_some());
        assert_eq!(tree.best_event.unwrap().d, 6);
        assert_eq!(tree.best_event.unwrap().delta_t, 12);
        assert_eq!(tree.state.d, 7);
        assert!(f32_slack(tree.state.integration, 100.0));
        assert!(f32_slack(tree.state.delta_t, 20.0));
        assert!(tree.alt.is_some());
        assert!(tree.alt.as_ref().unwrap().best_event.is_none());
        assert_eq!(tree.alt.as_ref().unwrap().state.d, 6);
        assert_eq!(tree.alt.as_ref().unwrap().state.integration, 36.0);
        assert!(f32_slack(tree.alt.as_ref().unwrap().state.delta_t, 7.2));

        tree.integrate(100.0, 20.0, &Continuous, &dtm);
        assert_eq!(tree.best_event.unwrap().d, 7);
        // Since we're casting, the delta t gets rounded down
        assert_eq!(tree.best_event.unwrap().delta_t, 25);
        assert_eq!(tree.state.d, 8);
        assert!(f32_slack(tree.state.integration, 200.0));
        assert!(f32_slack(tree.state.delta_t, 40.0));
        assert!(tree.alt.is_some());
        assert_eq!(tree.alt.as_ref().unwrap().state.d, 7);
        assert!(f32_slack(
            tree.alt.as_ref().unwrap().state.integration,
            72.0
        ));
        assert!(f32_slack(tree.alt.as_ref().unwrap().state.delta_t, 14.4));
        assert_eq!(tree.alt.as_ref().unwrap().best_event.unwrap().d, 6);
        assert_eq!(tree.alt.as_ref().unwrap().best_event.unwrap().delta_t, 12);
        assert!(tree.alt.as_ref().unwrap().alt.is_some());
        let alt_alt = tree.alt.as_ref().unwrap().alt.as_ref().unwrap();
        assert_eq!(alt_alt.state.d, 6);
        assert!(alt_alt.best_event.is_none());
        assert!(alt_alt.alt.is_none());
        assert!(f32_slack(alt_alt.state.integration, 8.0));
        assert!(f32_slack(alt_alt.state.delta_t, 1.6));

        // tree at this point
        // --node states are d, integration, delta_t
        // "best events" are (d, delta_t)
        //  ---------------------------------------------------------------------------8, 200, 40
        //                                        \
        //                                    (7,25)-----------------------------------7, 72, 14.4
        //                                                                  \
        //                                                             (6,12)----------6, 8, 1.6
        tree
    }

    fn make_tree2() -> PixelNode {
        let dtm = 10000;
        let mut tree = make_tree();
        tree.integrate(30.0, 34.0, &Continuous, &dtm);

        // Main node still not filled
        assert_eq!(tree.state.d, 8);
        assert!(f32_slack(tree.state.integration, 230.0));
        assert!(f32_slack(tree.state.delta_t, 74.0));

        let alt = tree.alt.as_ref().unwrap();
        assert_eq!(alt.state.d, 7);
        assert!(f32_slack(alt.state.integration, 102.0));
        assert!(f32_slack(alt.state.delta_t, 48.4));

        let alt = alt.alt.as_ref().unwrap();
        assert_eq!(alt.state.d, 6);
        assert!(f32_slack(alt.state.integration, 38.0));
        assert!(f32_slack(alt.state.delta_t, 35.6));

        //  ------------------------------------------------------------8, 230, 74
        //                  \
        //              (7,25)------------------------------------------7, 102, 48.4
        //                                         \
        //                                    (6,12)--------------------6, 38, 35.6

        tree.integrate(26.0, 34.0, &Continuous, &dtm);
        // Main node just filled
        assert_eq!(tree.state.d, 9);
        assert!(f32_slack(tree.state.integration, 256.0));
        assert!(f32_slack(tree.state.delta_t, 108.0));

        assert_eq!(tree.best_event.unwrap().d, 8);
        assert_eq!(tree.best_event.unwrap().delta_t, 108);

        let alt = tree.alt.as_ref().unwrap();
        assert_eq!(alt.state.d, 4);
        assert!(f32_slack(alt.state.integration, 0.0));
        assert!(f32_slack(alt.state.delta_t, 0.0));
        assert!(alt.best_event.is_none());
        assert!(alt.alt.is_none());

        //  ---------------------------9, 256, 108
        //                  \
        //              (8,108)--------4, 0, 0
        tree
    }

    #[test]
    fn test_make_tree() {
        make_tree();
    }

    #[test]
    fn test_make_tree2() {
        make_tree2();
    }

    #[test]
    fn test_pop_best_states() {
        let mut tree = make_tree();
        let events = tree.pop_best_events();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].d, 7);
        assert_eq!(events[0].delta_t, 25);
        assert_eq!(events[1].d, 6);
        assert_eq!(events[1].delta_t, 12);
        assert_eq!(tree.state.d, 6);
        assert!(f32_slack(tree.state.integration, 8.0));
        assert!(f32_slack(tree.state.delta_t, 1.6));
    }

    #[test]
    fn test_pop_best_states2() {
        let mut tree = make_tree2();
        let events = tree.pop_best_events();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].d, 8);
        assert_eq!(events[0].delta_t, 108);
        assert_eq!(tree.state.d, 4);
        assert!(f32_slack(tree.state.integration, 0.0));
        assert!(f32_slack(tree.state.delta_t, 0.0));
    }

    #[test]
    fn test_d_max() {
        // 1048576
        let dtm = 10000;
        let mut tree = PixelNode::new(1048500.0);
        let need_to_pop = tree.integrate(1048500.0, 1000.0, &Continuous, &dtm);
        assert!(need_to_pop);
        let events = tree.pop_best_events();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].d, 19);
        assert_eq!(events[0].delta_t, 500);
        assert!(f32_slack(tree.state.integration, 524212.0));
        // let need_to_pop = tree.integrate(1048500.0, 1000.0);
        // assert!(need_to_pop);
        // let events = tree.pop_best_events();
        // assert_eq!(events.len(), 2);
        // assert_eq!(events[0].d, 20);
        // assert_eq!(events[1].d, 19);
        // assert_eq!(tree.state.d, 19);
        // assert!(f32_slack(tree.state.integration, 524136.0));
    }

    fn f32_slack(num0: f32, num1: f32) -> bool {
        let slack = 0.1e-3;
        if num1 - slack <= num0 && num1 + slack >= num0 {
            return true;
        }
        return false;
    }
}
