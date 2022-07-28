use std::collections::VecDeque;
use crate::{D_SHIFT, DeltaT, Event, EventCoordless};

#[derive(Debug, Clone, Default)]
#[repr(C)]
pub struct FramePixel {
    pub(crate) running_ts: u32,
    ticks_per_frame: u32,
    original_delta_t_max: DeltaT,
    last_filled_frame: u32,
    pub(crate) frame_values: VecDeque<FrameElement>,
    already_filled: bool,
    target_frame: u32,
    repeat: u32,
    intensity: Gray16le,
    pub(crate) last_event_read: EventCoordless,
    pub(crate) target_event_read: Option<EventCoordless>,
    pub(crate) target_events_read_for_all: VecDeque<Option<(u32, EventCoordless)>>,  // TODO: this is such bad form
}
#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct Gray16le {
    pub intensity: u16,
}

#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct FrameElement {
    pub output_16le: Gray16le,
    pub repeat: u16,
}

impl FramePixel {
    pub fn init(&mut self, ticks_per_frame: u32, original_delta_t_max: DeltaT) {
        self.ticks_per_frame = ticks_per_frame;
        self.original_delta_t_max = original_delta_t_max;
        self.frame_values = VecDeque::with_capacity(60);
        self.target_event_read = None;
    }

    pub fn check_if_filled(&self, current_frame: u32) -> bool {
        self.last_filled_frame >= current_frame
    }

    // TODO: Make a separate trait for getting all frames vs. a single frame
    pub fn ingest_event_for_all(&mut self, event: &Event, current_frame: u32) -> bool {
        self.already_filled = self.last_filled_frame >= current_frame;
        self.running_ts += event.delta_t;

        // If this event constitutes a new frame interval
        // Only do something with it if that's the case (i.e., it's the very first event
        // in the frame)
        if ((self.running_ts - 1) / self.ticks_per_frame) + 1 > self.last_filled_frame {
            // TODO: deal with empty events

            match event.d {
                255 => {
                    // Just ignore this event
                    // panic!("Unexpected event")
                },
                _ => {
                    self.intensity = self.get_event_intensity_as_u16(&event);
                    // self.frame_values.push_back(FrameElement{ output_16le: self.intensity, repeat: self.repeat as u16});
                    // if self.frame_values.len() > self.original_delta_t_max as usize / self.ticks_per_frame as usize + 1 {
                    //     panic!("TOO LONG");
                    // }

                    // if (self.running_ts % self.ticks_per_frame) == 0, then don't add 1
                    self.last_filled_frame = ((self.running_ts - 1) / self.ticks_per_frame) + 1;


                    self.target_events_read_for_all.push_back(Some((self.last_filled_frame,EventCoordless {d: event.d, delta_t: event.delta_t})));
                }
            }
        }
        return self.last_filled_frame >= current_frame && !self.already_filled
    }

    fn get_event_intensity_as_u16(&mut self, event: &Event) -> Gray16le {
        return Gray16le {intensity:
        (((D_SHIFT[event.d as usize] as f32) / (u8::MAX as f32))
            * (self.ticks_per_frame as f32 / event.delta_t as f32)
            * u16::MAX as f32) as u16}

    }
}