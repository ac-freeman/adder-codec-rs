use crate::codec::compressed::blocks::block::BlockEvents;
use crate::codec::compressed::blocks::{
    DResidual, DeltaTResidual, TResidual, BLOCK_SIZE_AREA, D_ENCODE_NO_EVENT,
};
use crate::Mode::FramePerfect;
use crate::{AbsoluteT, DeltaT, EventCoordless, Mode, D};

pub static D_RESIDUALS_EMPTY: [DResidual; BLOCK_SIZE_AREA] = [D_ENCODE_NO_EVENT; BLOCK_SIZE_AREA];

/// Keeps track of the actual and predicted (reconstructed) times of past events, and gets the next
/// prediction residual
pub struct PredictionModel {
    /// Holds the true last t
    pub t_memory: [AbsoluteT; BLOCK_SIZE_AREA],

    /// Holds (reconstructed) delta_t values, regardless of time mode
    pub event_memory: [EventCoordless; BLOCK_SIZE_AREA],

    /// Holds the reconstructed last t
    pub t_recon: [AbsoluteT; BLOCK_SIZE_AREA],
    // TODO: Make the above three private
    /// The encoded d_residuals. Stored here so that we can recycle the memory.
    d_residuals: [DResidual; BLOCK_SIZE_AREA],

    dt_pred_residuals: [DeltaTResidual; BLOCK_SIZE_AREA],

    /// The residuals for the events' delta_t predictions. This is what actually gets arithmetic encoded.
    dt_pred_residuals_i16: [i16; BLOCK_SIZE_AREA],

    pub time_modulation_mode: Mode,
}

impl PredictionModel {
    pub fn new(time_modulation_mode: Mode) -> Self {
        PredictionModel {
            t_memory: [0; BLOCK_SIZE_AREA],
            event_memory: [Default::default(); BLOCK_SIZE_AREA],
            t_recon: [0; BLOCK_SIZE_AREA],
            d_residuals: D_RESIDUALS_EMPTY,
            dt_pred_residuals: [0; BLOCK_SIZE_AREA],
            dt_pred_residuals_i16: [0; BLOCK_SIZE_AREA],
            time_modulation_mode,
        }
    }

    pub fn override_memory(
        &mut self,
        event_memory: [EventCoordless; BLOCK_SIZE_AREA],
        t_recon: [AbsoluteT; BLOCK_SIZE_AREA],
    ) {
        self.event_memory = event_memory;
        self.t_recon = t_recon;
    }

    fn reset_memory(&mut self) {
        self.t_memory = [0; BLOCK_SIZE_AREA];
        self.event_memory = [Default::default(); BLOCK_SIZE_AREA];
        self.t_recon = [0; BLOCK_SIZE_AREA];
    }

    fn reset_residuals(&mut self) {
        // self.t_memory = [0; BLOCK_SIZE_AREA];
        // self.event_memory = [Default::default(); BLOCK_SIZE_AREA],
        self.d_residuals = D_RESIDUALS_EMPTY;
        self.dt_pred_residuals = [0; BLOCK_SIZE_AREA];
        self.dt_pred_residuals_i16 = [0; BLOCK_SIZE_AREA];
    }

    pub(crate) fn forward_intra_prediction(
        &mut self,
        mut sparam: u8,
        dtm: DeltaT,
        dt_ref: DeltaT,
        events: &BlockEvents,
    ) -> (
        AbsoluteT,
        D,
        &[DResidual; BLOCK_SIZE_AREA],
        &[i16; BLOCK_SIZE_AREA],
        u8,
    ) {
        self.reset_residuals();
        self.reset_memory();
        let mut init = false;
        let mut start = EventCoordless { d: 0, delta_t: 0 };

        let mut max_t_resid = 0;

        for (idx, event_opt) in events.iter().enumerate() {
            if let Some(prev) = event_opt {
                // If this is the first event encountered, then encode it directly
                if !init {
                    self.event_memory[idx].d = prev.d;
                    self.event_memory[idx].delta_t = 0;
                    debug_assert!(self.event_memory[idx].delta_t <= dtm);
                    init = true;
                    self.t_memory[idx] = prev.t();
                    frame_perfect_alignment(
                        self.time_modulation_mode,
                        &mut self.t_memory[idx],
                        dt_ref,
                    );

                    self.t_recon[idx] = self.t_memory[idx];

                    start = *prev;
                }

                // Get the prediction residual for the next event and store it
                for (next_idx, next_event_opt) in events.iter().skip(idx + 1).enumerate() {
                    if let Some(next) = next_event_opt {
                        let abs_next_idx = next_idx + idx + 1;
                        self.event_memory[abs_next_idx].d = next.d;
                        // self.event_memory[abs_next_idx].delta_t =
                        //     next.t() - self.t_memory[abs_next_idx];
                        self.event_memory[abs_next_idx].delta_t = 0;
                        debug_assert!(self.event_memory[idx].delta_t <= dtm);

                        let d_resid = next.d as DResidual - start.d as DResidual;
                        let t_resid =
                            next.delta_t as DeltaTResidual - start.delta_t as DeltaTResidual;

                        self.d_residuals[abs_next_idx] = d_resid;
                        self.dt_pred_residuals[abs_next_idx] = t_resid;

                        self.t_memory[abs_next_idx] = next.t();
                        frame_perfect_alignment(
                            self.time_modulation_mode,
                            &mut self.t_memory[abs_next_idx],
                            dt_ref,
                        );
                        self.t_recon[next_idx + idx + 1] = self.t_memory[abs_next_idx];

                        if t_resid.abs() > max_t_resid {
                            max_t_resid = t_resid.abs();
                            if max_t_resid > dtm as i64 {
                                eprintln!(
                                    "max_t_resid: {}, next_dt: {}, start_dt: {}, ",
                                    max_t_resid, next.delta_t, start.delta_t
                                );
                            }
                        }
                        break;
                    }
                }
            } else {
                // In the case that this cube is on the spatial edge, of the plane, and doesn't have
                // data in every pixel
                break;
            }
        }

        // if max_t_resid is greater than 2^15, then we need to increase the sparam
        let num_places = max_t_resid.leading_zeros();
        if num_places + (sparam as u32) < 49 && max_t_resid > 0 {
            sparam = (49 - num_places) as u8;
        }

        // Quantize the T residuals
        for (t_resid, t_resid_i16) in self
            .dt_pred_residuals
            .iter()
            .zip(self.dt_pred_residuals_i16.iter_mut())
        {
            assert!(*t_resid >> sparam <= i16::MAX as i64);
            *t_resid_i16 = (*t_resid >> sparam) as i16;
        }

        self.event_memory[0].delta_t = 0; // TODO: This will cause bad prediction residuals for the first inter block

        (
            start.delta_t,
            start.d,
            &self.d_residuals,
            &self.dt_pred_residuals_i16,
            sparam,
        )
    }

    pub(crate) fn inverse_intra_prediction(
        &mut self,
        mut sparam: u8,
        dtm: DeltaT,
        dt_ref: DeltaT,
        start_t: AbsoluteT,
        start_d: D,
        d_residuals: [DResidual; BLOCK_SIZE_AREA],
        mut t_residuals: [i16; BLOCK_SIZE_AREA],
    ) -> BlockEvents {
        let mut events: [Option<EventCoordless>; BLOCK_SIZE_AREA] = [None; BLOCK_SIZE_AREA];
        let mut init = false;
        let mut start = EventCoordless {
            d: start_d,
            delta_t: start_t,
        };
        self.event_memory[0] = start;

        events[0] = Some(start);
        self.t_memory[0] = start_t;
        if self.time_modulation_mode == FramePerfect && self.t_memory[0] % dt_ref != 0 {
            self.t_memory[0] = ((self.t_memory[0] / dt_ref) + 1) * dt_ref;
        }
        self.t_recon[0] = self.t_memory[0];

        for ((idx, d_resid), t_resid) in d_residuals.iter().enumerate().zip(t_residuals.iter()) {
            if *d_resid != D_ENCODE_NO_EVENT {
                let next = EventCoordless {
                    d: (*d_resid + start.d as DResidual) as D,
                    delta_t: (((start.delta_t as DeltaTResidual) << sparam)
                        + ((*t_resid as DeltaTResidual) << sparam))
                        as DeltaT,
                };
                self.t_memory[idx] = next.t();
                frame_perfect_alignment(self.time_modulation_mode, &mut self.t_memory[idx], dt_ref);
                self.t_recon[idx] = self.t_memory[idx];

                self.event_memory[idx] = next;
                debug_assert!(self.event_memory[idx].delta_t <= dtm);
                events[idx] = Some(next);
            }
        }
        self.event_memory[0].delta_t = 0; // TODO: This will cause bad prediction residuals for the first inter block
        debug_assert!(self.event_memory[0].delta_t <= dtm);

        events
    }

    /// Get a block of inter-prediction residuals. `t_memory` should hold the previous absolute t
    /// values for each pixel in the block. If the previous block was also inter-coded, then this
    /// memory should be the _reconstructed_ t values after compression (to prevent temporal drift).
    /// In the end, we'll do intra-coding at the beginning of each dtm interval, so there's a guarantee
    /// that each pixel will have an event in the first block.
    pub(crate) fn forward_inter_prediction(
        &mut self,
        mut sparam: u8,
        dtm: DeltaT,
        dt_ref: DeltaT,
        events: &BlockEvents,
    ) -> (&[DResidual; 256], &[i16; 256], u8) {
        self.reset_residuals();
        let mut max_t_resid = 0;
        for ((idx, event_opt), event_mem) in
            events.iter().enumerate().zip(self.event_memory.iter_mut())
        {
            if let Some(next) = event_opt {
                // Get the d-residual
                let d_resid = d_residual(event_mem.d, next.d);
                // event_mem.d = next.d; // ??? TODO
                self.d_residuals[idx] = d_resid;

                let tmp = self.t_memory[idx];

                // The true delta_t
                let delta_t = next.t() - self.t_memory[idx];
                assert!(delta_t > 0);
                assert!(delta_t <= dtm);

                self.t_memory[idx] = next.t();
                frame_perfect_alignment(self.time_modulation_mode, &mut self.t_memory[idx], dt_ref);

                let dt_pred = predict_delta_t(event_mem, d_resid, dtm);

                // event_mem.delta_t = delta_t; // ???? TODO

                let dt_pred_residual = delta_t as DeltaTResidual - dt_pred as DeltaTResidual;
                self.dt_pred_residuals[idx] = dt_pred_residual;
                if dt_pred_residual.abs() > max_t_resid {
                    max_t_resid = dt_pred_residual.abs();
                    assert!(max_t_resid <= dtm as DeltaTResidual);
                    assert!(max_t_resid < 100000000);
                }
            }
        }

        // if max_t_resid is greater than 2^15, then we need to increase the sparam
        let num_places = max_t_resid.leading_zeros();
        if num_places + (sparam as u32) < 49 && max_t_resid > 0 {
            sparam = (49 - num_places) as u8;
        }

        // Quantize the T residuals
        for (t_resid, t_resid_i16) in self
            .dt_pred_residuals
            .iter()
            .zip(self.dt_pred_residuals_i16.iter_mut())
        {
            *t_resid_i16 = (*t_resid >> sparam) as i16;
            // assert!(t_resid_i16.abs() <= dtm as i16);
        }

        self.reconstruct_t_values(None, sparam, dtm, dt_ref);

        // TODO: temporary
        for (idx, t_recon) in self.t_recon.iter().enumerate() {
            if self.d_residuals[idx] != D_ENCODE_NO_EVENT {
                assert_eq!(self.t_memory[idx], *t_recon);
            }
        }

        (&self.d_residuals, &self.dt_pred_residuals_i16, sparam)
    }

    pub(crate) fn inverse_inter_prediction(
        &mut self,
        sparam: u8,
        dtm: DeltaT,
        dt_ref: DeltaT,
        d_residuals: [DResidual; BLOCK_SIZE_AREA],
        mut dt_pred_residuals_i16: [i16; BLOCK_SIZE_AREA],
    ) -> [Option<EventCoordless>; BLOCK_SIZE_AREA] {
        let mut events = [None; BLOCK_SIZE_AREA];

        self.d_residuals = d_residuals;
        self.dt_pred_residuals_i16 = dt_pred_residuals_i16;
        self.reconstruct_t_values(Some(&mut events), sparam, dtm, dt_ref);

        events
    }

    /// Reconstruct the t values from the t prediction residuals. Called by `forward_inter_prediction`
    ///
    fn reconstruct_t_values(
        &mut self,
        mut events: Option<&mut [Option<EventCoordless>; 256]>,
        sparam: u8,
        dtm: DeltaT,
        dt_ref: DeltaT,
    ) {
        for ((event_mem, t_resid_i16), (idx, d_resid)) in self
            .event_memory
            .iter_mut()
            .zip(self.dt_pred_residuals_i16.iter())
            .zip(self.d_residuals.iter().enumerate())
        {
            if *d_resid != D_ENCODE_NO_EVENT as i16 {
                // debug_assert!(event_mem.delta_t >= 0); // Sanity check
                // let mut event = EventCoordless { d, delta_t: 0 }
                let t_resid = ((*t_resid_i16 as DeltaTResidual) << sparam);
                let mut dt_pred = predict_delta_t(event_mem, *d_resid, dtm);

                update_values_from_prediction(
                    event_mem,
                    &mut self.t_recon[idx],
                    dt_pred,
                    t_resid,
                    *d_resid,
                    dtm,
                );

                self.t_memory[idx] = self.t_recon[idx];

                match events {
                    None => {}
                    Some(events_mut) => {
                        // TODO: Write this cleaner
                        let event = EventCoordless {
                            d: event_mem.d,
                            delta_t: self.t_recon[idx],
                        };
                        events_mut[idx] = Some(event);
                        events = Some(events_mut);
                    }
                }
            }
        }
    }
}

#[inline(always)]
fn d_residual(d0: D, d1: D) -> DResidual {
    d1 as DResidual - d0 as DResidual
}

#[inline(always)]
fn t_residual(t0: AbsoluteT, t1: AbsoluteT) -> TResidual {
    t1 - t0
}

#[inline(always)]
fn delta_t_residual(t0: DeltaTResidual, t1: DeltaTResidual) -> DeltaTResidual {
    t1 - t0
}

#[inline]
fn predict_delta_t(event_memory: &mut EventCoordless, d_resid: DResidual, dtm: DeltaT) -> DeltaT {
    let mut dt_pred = match d_resid > 0 {
        true => {
            if d_resid < 8 {
                event_memory.delta_t << d_resid
            } else {
                event_memory.delta_t
            }
        }
        false => {
            if d_resid > -8 {
                event_memory.delta_t >> -d_resid
            } else {
                event_memory.delta_t
            }
        }
    };
    if dt_pred > dtm {
        dt_pred = event_memory.delta_t;
    }
    dt_pred
}

#[inline]
fn frame_perfect_alignment(time_modulation_mode: Mode, t_value: &mut AbsoluteT, dt_ref: DeltaT) {
    if time_modulation_mode == FramePerfect && *t_value % dt_ref != 0 {
        *t_value = ((*t_value / dt_ref) + 1) * dt_ref;
    }
}

fn update_values_from_prediction(
    event_memory: &mut EventCoordless,
    t_recon: &mut AbsoluteT,
    dt_pred: DeltaT,
    dt_pred_residual: DeltaTResidual,
    d_residual: DResidual,
    dtm: DeltaT,
) {
    let d = (event_memory.d as DResidual + d_residual) as D;
    let recon_t =
        (*t_recon as DeltaTResidual + dt_pred as DeltaTResidual + dt_pred_residual) as AbsoluteT;
    event_memory.delta_t = recon_t - *t_recon;
    event_memory.d = d;
    assert!(event_memory.delta_t <= dtm);
    // self.event_memory[idx].d = d; TODO?
    *t_recon = recon_t;
}

#[cfg(test)]
mod tests {
    use crate::codec::compressed::blocks::block::BlockEvents;
    use crate::codec::compressed::blocks::BLOCK_SIZE_AREA;
    use crate::Mode::FramePerfect;
    use crate::{Coord, DeltaT, Event};
    use rand::prelude::StdRng;
    use rand::{Rng, SeedableRng};

    #[test]
    fn test_prediction_lossless() {
        let dtm = 255;
        let mut events = get_random_events(Some(7), BLOCK_SIZE_AREA, 16, 16, 1, dtm);
        let mut events_coordless = events
            .iter()
            .map(|event| super::EventCoordless {
                d: event.d,
                delta_t: event.delta_t,
            })
            .collect::<Vec<_>>();
        let block_events: BlockEvents = events_coordless
            .iter()
            .map(|event| Some(*event))
            .collect::<Vec<_>>()
            .try_into()
            .unwrap();

        // Do the forward intra prediction

        let mut forward_prediction_model = super::PredictionModel::new(FramePerfect);
        let (start_t, start_d, d_resids, t_resids_i16, sparam) =
            forward_prediction_model.forward_intra_prediction(0, dtm, 255, &block_events);
        let ref_start_t = events[0].delta_t;
        assert_eq!(start_t, ref_start_t);
        assert_eq!(start_d, events[0].d);

        // Do the inverse intra prediction
        let mut inverse_prediction_model = super::PredictionModel::new(FramePerfect);
        let reconstructed_events = inverse_prediction_model.inverse_intra_prediction(
            sparam,
            dtm,
            255,
            start_t,
            start_d,
            *d_resids,
            *t_resids_i16,
        );

        // Check that the reconstructed events are the same as the original events
        for (recon_event, event) in reconstructed_events.iter().zip(block_events.iter()) {
            assert!(recon_event.is_some());
            assert!(event.is_some());
            assert_eq!(recon_event.unwrap().d, event.unwrap().d);
            let recon_t = recon_event.unwrap().delta_t;
            let ref_t = event.unwrap().delta_t;
            assert_eq!(recon_t, ref_t);
        }

        // Check that the models have the same state
        assert_eq!(
            forward_prediction_model.t_memory,
            inverse_prediction_model.t_memory
        );
        for (idx, event) in forward_prediction_model.event_memory.iter().enumerate() {
            assert_eq!(event.d, inverse_prediction_model.event_memory[idx].d);
            assert_eq!(
                event.delta_t,
                inverse_prediction_model.event_memory[idx].delta_t
            );
        }
        assert_eq!(
            forward_prediction_model.event_memory,
            inverse_prediction_model.event_memory
        );

        // Get events for inter block
        let mut inter_block_events = block_events.clone();
        inter_block_events.reverse();
        for (idx, inter_event) in inter_block_events.iter_mut().enumerate() {
            // Ensure that the events have a bigger timestamp than the previous ones
            if let Some(event) = inter_event {
                event.delta_t += forward_prediction_model.t_memory[idx];
            }
        }

        // Do the forward inter prediction
        let (d_resids, dt_resids_i16, sparam) =
            forward_prediction_model.forward_inter_prediction(0, dtm, 255, &inter_block_events);
        // Check that the residuals are correct
        for ((idx, new_event), ref_event) in inter_block_events
            .iter()
            .enumerate()
            .zip(block_events.iter())
        {
            assert_eq!(
                d_resids[idx],
                new_event.unwrap().d as i16 - ref_event.unwrap().d as i16
            );
        }

        // Do the inverse inter prediction
        let reconstructed_events = inverse_prediction_model.inverse_inter_prediction(
            sparam,
            dtm,
            255,
            *d_resids,
            *dt_resids_i16,
        );

        // Check that the models have the same state
        for ((idx, forward_t), inverse_t) in forward_prediction_model
            .t_memory
            .iter()
            .enumerate()
            .zip(inverse_prediction_model.t_memory.iter())
        {
            assert_eq!(forward_t, inverse_t);
        }
        assert_eq!(
            forward_prediction_model.t_memory,
            inverse_prediction_model.t_memory
        );
        for (idx, event) in forward_prediction_model.event_memory.iter().enumerate() {
            assert_eq!(event.d, inverse_prediction_model.event_memory[idx].d);
            assert_eq!(
                event.delta_t,
                inverse_prediction_model.event_memory[idx].delta_t
            );
        }
        assert_eq!(
            forward_prediction_model.event_memory,
            inverse_prediction_model.event_memory
        );

        // Check that the reconstructed events are the same as the original events
        for ((idx, recon_event), event) in reconstructed_events
            .iter()
            .enumerate()
            .zip(inter_block_events.iter())
        {
            assert!(recon_event.is_some());
            assert!(event.is_some());
            assert_eq!(recon_event.unwrap().d, event.unwrap().d);
            let recon_t = recon_event.unwrap().delta_t;
            let ref_t = event.unwrap().delta_t;
            assert_eq!(recon_t, ref_t);
        }

        let prev_events = inter_block_events.clone();
        // Get events for inter block
        let mut inter_block_events = block_events.clone();
        inter_block_events.reverse();
        for (idx, inter_event) in inter_block_events.iter_mut().enumerate() {
            // Ensure that the events have a bigger timestamp than the previous ones
            if let Some(event) = inter_event {
                event.delta_t += forward_prediction_model.t_memory[idx];
            }
        }

        // Do the forward inter prediction
        let (d_resids, dt_resids_i16, sparam) =
            forward_prediction_model.forward_inter_prediction(0, dtm, 255, &inter_block_events);
        // Check that the residuals are correct
        for ((idx, new_event), ref_event) in inter_block_events
            .iter()
            .enumerate()
            .zip(prev_events.iter())
        {
            assert_eq!(
                d_resids[idx],
                new_event.unwrap().d as i16 - ref_event.unwrap().d as i16
            );
        }

        // Do the inverse inter prediction
        let reconstructed_events = inverse_prediction_model.inverse_inter_prediction(
            sparam,
            dtm,
            255,
            *d_resids,
            *dt_resids_i16,
        );

        // Check that the reconstructed events are the same as the original events
        for ((idx, recon_event), event) in reconstructed_events
            .iter()
            .enumerate()
            .zip(inter_block_events.iter())
        {
            assert!(recon_event.is_some());
            assert!(event.is_some());
            assert_eq!(recon_event.unwrap().d, event.unwrap().d);
            let recon_t = recon_event.unwrap().delta_t;
            let ref_t = event.unwrap().delta_t;
            assert_eq!(recon_t, ref_t);
        }
    }

    fn get_random_events(
        seed: Option<u64>,
        num: usize,
        width: u16,
        height: u16,
        channels: u8,
        dtm: DeltaT,
    ) -> Vec<Event> {
        let mut rng = match seed {
            None => StdRng::from_rng(rand::thread_rng()).unwrap(),
            Some(num) => StdRng::seed_from_u64(num),
        };
        let mut events = Vec::with_capacity(num);
        for _ in 0..num {
            let event = Event {
                coord: Coord {
                    x: rng.gen::<u16>() % width,
                    y: rng.gen::<u16>() % height,
                    c: Some(rng.gen::<u8>() % channels),
                },
                d: rng.gen::<u8>(),
                delta_t: rng.gen_range(1..dtm),
            };
            events.push(event);
        }
        events
    }
}
