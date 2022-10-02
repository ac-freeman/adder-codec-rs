use crate::transcoder::event_pixel::{Intensity, D};
use crate::transcoder::event_pixel_tree::Mode::{Continuous, FramePerfect};
use crate::{Coord, DeltaT, Event, EventCoordless, SourceCamera, D_MAX, D_SHIFT};
use generational_arena::{Arena, Index};
use smallvec::{smallvec, SmallVec};
use std::collections::VecDeque;
use std::mem;

pub(crate) enum Mode {
    FramePerfect,
    Continuous,
}

#[derive(Copy, Clone, Debug)]
struct PixelState {
    d: D,
    integration: Intensity,
    delta_t: f32,
}

#[derive(Clone)]
pub struct PixelNode {
    /// Will have the smaller D value
    alt: Option<()>,

    state: PixelState,
    best_event: Option<Event>,
}

pub struct PixelArena {
    pub arena: SmallVec<[PixelNode; 5]>,
    length: usize,
    pub coord: Coord,
    pub base_val: u8,
}

impl PixelArena {
    pub(crate) fn new(start_intensity: Intensity, coord: Coord) -> PixelArena {
        // let mut arena = VecDeque::with_capacity(5);
        let mut arena = smallvec![];
        arena.push(PixelNode::new(start_intensity));
        PixelArena {
            arena,
            length: 1,
            coord,
            base_val: 0,
        }
    }

    /// Pop just the topmost event. Should be called only when dtm is reached for main node
    pub fn pop_top_event(&mut self, next_intensity: Option<Intensity>) -> Event {
        let mut root = &mut self.arena[0];
        match root.best_event {
            None => {
                if root.state.integration == 0.0 && root.state.delta_t > 0.0 {
                    // If the integration is 0, we need to forcefully fire an event where d=254
                    let ret_event = Event {
                        coord: self.coord,
                        d: 254,
                        delta_t: root.state.delta_t as DeltaT,
                    };
                    root.state.delta_t = 0.0;
                    match next_intensity {
                        None => {}
                        Some(intensity) => root.state.d = get_d_from_intensity(intensity),
                    }
                    ret_event
                } else {
                    panic!("No best event! TODO: handle it")
                }
            }
            Some(event) => {
                assert!(self.length > 1);
                self.arena.remove(0);
                // self.arena.pop_front();
                self.length -= 1;

                // let alt = self.alt.as_deref_mut().unwrap();
                // *self = alt.clone();
                event
            }
        }
    }

    /// Recursively pop all the alt events
    pub fn pop_best_events(&mut self, next_intensity: Option<Intensity>) -> Vec<Event> {
        let mut events = Vec::new();
        for node_idx in 0..self.length {
            match self.arena[node_idx].best_event {
                None => {}
                Some(event) => events.push(event),
            }
        }
        self.arena.swap(0, self.length - 1);
        debug_assert!(self.arena[0].alt.is_none());
        self.length = 1;
        // self.arena.drain(..self.arena.len() - 1);
        // let mut res = self.pop_and_reset_state(0);
        // let mut root = &mut self.arena[0];
        // root.state = res.1;
        // if root.state.delta_t > 0.0 {
        //     // dbg!(self.state);
        //
        //     if root.state.integration == 0.0 {
        //         // If the integration is 0, we need to forcefully fire an event where d=254
        //         res.0.push(EventCoordless {
        //             d: 254,
        //             delta_t: root.state.delta_t as DeltaT,
        //         });
        //     } else {
        //         // TODO: sanity check assertions
        //     }
        //
        //     // If framed source and we want to ignore the rest of the integration for the last
        //     // frame, just clear the state.
        // }

        match next_intensity {
            None => {}
            // TODO: match on mode instead. This is disjoint.
            Some(intensity) => {
                self.arena[0].state.d = get_d_from_intensity(intensity);
                self.arena[0].state.integration = 0.0;
                self.arena[0].state.delta_t = 0.0;
            }
        };
        events

        // root.alt = None; // Free the memory for the alternate branch
        // root.best_event = None;
        // res.0
    }

    // fn pop_and_reset_state(&mut self, index: usize) -> (Vec<EventCoordless>, PixelState) {
    //     let node = &self.arena[index];
    //     match node.best_event {
    //         None => {
    //             // panic!("No best event! TODO: handle it")
    //             // if self.state.integration > 0.0 {
    //             //     dbg!(self.state);
    //             // }
    //             (vec![], node.state.clone())
    //         }
    //         Some(event) => {
    //             let mut ret = vec![event];
    //
    //             let res = match node.alt.is_some() {
    //                 false => (vec![], node.state.clone()),
    //                 true => {
    //                     let rec = self.pop_and_reset_state(index + 1);
    //                     self.arena.pop_back();
    //                     rec
    //                 }
    //             };
    //             ret.extend(res.0);
    //             (ret, res.1)
    //         }
    //     }
    // }

    // Integrates the intensity. Returns bool indicating whether or not the topmost event MUST be popped
    // or else risk losing accuracy. Should only return true when d=D_MAX, which should be
    // extremely rare, or when delta_t_max is hit
    pub fn integrate(
        &mut self,
        index: usize,
        mut intensity: Intensity,
        mut time: f32,
        mode: &Mode,
        dtm: &DeltaT,
    ) -> bool {
        // debug_assert!(intensity <= 255.0);
        // debug_assert_ne!(intensity, 0.0);
        // debug_assert_ne!(time, 0.0);
        // assert_ne!(self.state.d, D_MAX);
        let mut idx = 0;
        loop {
            match self.integrate_main(idx, intensity, time, mode) {
                None => {}
                Some((next_intensity, next_time)) => {
                    // self.arena.drain(idx + 1..);
                    match self.arena.len() > idx + 1 {
                        true => self.arena[idx + 1] = PixelNode::new(intensity),
                        false => {
                            self.arena.push(PixelNode::new(intensity));
                        }
                    }
                    self.length = idx + 2;
                    self.arena[idx].alt = Some(());
                    intensity = next_intensity;
                    time = next_time;
                }
            }
            idx += 1;
            if idx >= self.length {
                break;
            }
        }
        debug_assert!(self.length <= self.arena.len());
        assert!(self.length > 0);

        // match self.integrate_main(index, intensity, time, mode) {
        //     None => {
        //         // Only should do when the main has not just fired and created the alt
        //         if self.arena[index].alt.is_some() {
        //             self.integrate(index + 1, intensity, time, mode, dtm);
        //         }
        //     }
        //     Some((intensity, time)) => {
        //         self.arena[index].alt = Some(());
        //         self.integrate(index + 1, intensity, time, mode, dtm);
        //     }
        // }
        // debug_assert!(
        //     D_SHIFT[self.arena[index].state.d as usize] as Intensity
        //         > self.arena[index].state.integration
        // );
        return self.arena[0].state.d == D_MAX || self.arena[0].state.delta_t as DeltaT >= *dtm;
        // && self
        //     .best_event
        //     .unwrap_or(EventCoordless { d: 0, delta_t: 0 })
        //     .d
        //     == D_MAX;
    }

    pub fn integrate_main(
        &mut self,
        index: usize,
        intensity: Intensity,
        time: f32,
        mode: &Mode,
    ) -> Option<(Intensity, f32)> {
        let node = &mut self.arena[index];
        return if node.state.integration + intensity >= D_SHIFT[node.state.d as usize] as f32 {
            let prop =
                (D_SHIFT[node.state.d as usize] as f32 - node.state.integration) as f32 / intensity;
            assert!(prop > 0.0);
            node.best_event = Some(Event {
                coord: self.coord,
                d: node.state.d,
                delta_t: (node.state.delta_t + time * prop) as DeltaT,
            });

            // Increase d to prepare for the next integration of this pixel
            if node.state.d < D_MAX {
                node.state.integration += intensity;
                node.state.delta_t += time;
                loop {
                    node.state.d += 1;
                    if D_SHIFT[node.state.d as usize] > node.state.integration as u32 {
                        break;
                    }
                }
            } else {
                // dbg!(self.state.integration);
            }

            if intensity - (intensity * prop) >= 0.0 {
                // For a framed source, we need to return 0,0 for intensity,time.
                // This lets us preserve the spatially-coherent intensities, especially for color
                // transcode.
                // match self.arena[index].alt {
                //     // self.arena.remove(self.arena[index].alt.unwrap());
                //     None => {
                //         self.arena.push_back(PixelNode::new(intensity));
                //     }
                //     Some(_) => self.arena[index + 1] = PixelNode::new(intensity),
                // }

                return Some(match mode {
                    FramePerfect => (0.0, 0.0),
                    Continuous => (intensity - (intensity * prop), time - (time * prop)),
                });
            }
            None
        } else {
            node.state.integration += intensity;
            node.state.delta_t += time;
            None
        };
    }
}

fn get_d_from_intensity(intensity: Intensity) -> D {
    match intensity > 0.0 {
        true => fast_math::log2_raw(intensity) as D,
        false => 0,
    }
}

impl PixelNode {
    pub fn new(start_intensity: Intensity) -> PixelNode {
        let start_d = get_d_from_intensity(start_intensity);
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
}

#[cfg(test)]
mod tests {
    // Note this useful idiom: importing names from outer (for mod tests) scope.
    use super::*;

    fn make_tree() -> PixelArena {
        let dtm = 10000;
        let mut tree = PixelArena::new(
            100.0,
            Coord {
                x: 0,
                y: 0,
                c: None,
            },
        );
        assert_eq!(tree.arena[0].state.d, 6);
        tree.integrate(0, 100.0, 20.0, &Continuous, &dtm);
        assert!(tree.arena[0].best_event.is_some());
        assert_eq!(tree.arena[0].best_event.unwrap().d, 6);
        assert_eq!(tree.arena[0].best_event.unwrap().delta_t, 12);
        assert_eq!(tree.arena[0].state.d, 7);
        assert!(f32_slack(tree.arena[0].state.integration, 100.0));
        assert!(f32_slack(tree.arena[0].state.delta_t, 20.0));
        assert!(tree.arena[0].alt.is_some());
        assert!(tree.arena[1].best_event.is_none());
        assert_eq!(tree.arena[1].state.d, 6);
        assert_eq!(tree.arena[1].state.integration, 36.0);
        assert!(f32_slack(tree.arena[1].state.delta_t, 7.2));

        tree.integrate(0, 100.0, 20.0, &Continuous, &dtm);
        assert_eq!(tree.arena[0].best_event.unwrap().d, 7);
        // Since we're casting, the delta t gets rounded down
        assert_eq!(tree.arena[0].best_event.unwrap().delta_t, 25);
        assert_eq!(tree.arena[0].state.d, 8);
        assert!(f32_slack(tree.arena[0].state.integration, 200.0));
        assert!(f32_slack(tree.arena[0].state.delta_t, 40.0));
        assert!(tree.arena[0].alt.is_some());
        assert_eq!(tree.arena[1].state.d, 7);
        assert!(f32_slack(tree.arena[1].state.integration, 72.0));
        assert!(f32_slack(tree.arena[1].state.delta_t, 14.4));
        assert_eq!(tree.arena[1].best_event.unwrap().d, 6);
        assert_eq!(tree.arena[1].best_event.unwrap().delta_t, 12);
        assert!(tree.arena[1].alt.is_some());
        let alt_alt = &tree.arena[2];
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

    fn make_tree2() -> PixelArena {
        let dtm = 10000;
        let mut tree = make_tree();
        tree.integrate(0, 30.0, 34.0, &Continuous, &dtm);

        {
            let root = &tree.arena[0];
            // Main node still not filled
            assert_eq!(root.state.d, 8);
            assert!(f32_slack(root.state.integration, 230.0));
            assert!(f32_slack(root.state.delta_t, 74.0));

            let alt = &tree.arena[1];
            assert_eq!(alt.state.d, 7);
            assert!(f32_slack(alt.state.integration, 102.0));
            assert!(f32_slack(alt.state.delta_t, 48.4));

            let alt = &tree.arena[2];
            assert_eq!(alt.state.d, 6);
            assert!(f32_slack(alt.state.integration, 38.0));
            assert!(f32_slack(alt.state.delta_t, 35.6));
        }

        //  ------------------------------------------------------------8, 230, 74
        //                  \
        //              (7,25)------------------------------------------7, 102, 48.4
        //                                         \
        //                                    (6,12)--------------------6, 38, 35.6

        tree.integrate(0, 26.0, 34.0, &Continuous, &dtm);
        // Main node just filled
        assert_eq!(tree.arena[0].state.d, 9);
        assert!(f32_slack(tree.arena[0].state.integration, 256.0));
        assert!(f32_slack(tree.arena[0].state.delta_t, 108.0));

        assert_eq!(tree.arena[0].best_event.unwrap().d, 8);
        assert_eq!(tree.arena[0].best_event.unwrap().delta_t, 108);

        let alt = &tree.arena[1];
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
        let events = tree.pop_best_events(None);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].d, 7);
        assert_eq!(events[0].delta_t, 25);
        assert_eq!(events[1].d, 6);
        assert_eq!(events[1].delta_t, 12);
        assert_eq!(tree.arena[0].state.d, 6);
        assert!(f32_slack(tree.arena[0].state.integration, 8.0));
        assert!(f32_slack(tree.arena[0].state.delta_t, 1.6));
    }

    #[test]
    fn test_pop_best_states2() {
        let mut tree = make_tree2();
        let events = tree.pop_best_events(None);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].d, 8);
        assert_eq!(events[0].delta_t, 108);
        assert_eq!(tree.arena[0].state.d, 4);
        assert!(f32_slack(tree.arena[0].state.integration, 0.0));
        assert!(f32_slack(tree.arena[0].state.delta_t, 0.0));
    }

    #[test]
    fn test_d_max() {
        // 1048576
        let dtm = 10000;
        let mut tree = PixelArena::new(
            1048500.0,
            Coord {
                x: 0,
                y: 0,
                c: None,
            },
        );
        let need_to_pop = tree.integrate(0, 1048500.0, 1000.0, &Continuous, &dtm);
        assert!(need_to_pop);
        let events = tree.pop_best_events(None);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].d, 19);
        assert_eq!(events[0].delta_t, 500);
        assert!(f32_slack(tree.arena[0].state.integration, 524212.0));
        // let need_to_pop = tree.integrate(1048500.0, 1000.0);
        // assert!(need_to_pop);
        // let events = tree.pop_best_events();
        // assert_eq!(events.len(), 2);
        // assert_eq!(events[0].d, 20);
        // assert_eq!(events[1].d, 19);
        // assert_eq!(tree.state.d, 19);
        // assert!(f32_slack(tree.state.integration, 524136.0));
    }

    #[test]
    fn test_dtm() {
        let dtm = 240000;
        let mut tree = PixelArena::new(
            245.0,
            Coord {
                x: 0,
                y: 0,
                c: None,
            },
        );
        for i in 0..47 {
            tree.integrate(0, 245.0, 5000.0, &FramePerfect, &dtm);
        }
        let need_to_pop = tree.integrate(0, 245.0, 5000.0, &FramePerfect, &dtm);
        assert!(need_to_pop);
        let ret = tree.pop_top_event(Some(245.0));
        assert_eq!(tree.arena[0].state.delta_t, 70000.0)
    }

    fn f32_slack(num0: f32, num1: f32) -> bool {
        let slack = 0.1e-3;
        if num1 - slack <= num0 && num1 + slack >= num0 {
            return true;
        }
        return false;
    }
}
