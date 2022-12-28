use crate::framer::event_framer::SourceType;
use crate::transcoder::source::video::FramedViewMode;
use crate::{DeltaT, Event, EventCoordless, Intensity, D_SHIFT};

pub trait FrameValue {
    type Output;
    fn get_frame_value(
        event: &Event,
        source_type: SourceType,
        i1: DeltaT,
        practical_d_max: f32,
        delta_t_max: DeltaT,
        view_mode: FramedViewMode,
    ) -> Self::Output;

    fn max_f32() -> f32;
}

impl FrameValue for EventCoordless {
    type Output = EventCoordless;
    fn get_frame_value(
        event: &Event,
        _source_type: SourceType,
        _tpf: DeltaT,
        _practical_d_max: f32,
        _delta_t_max: DeltaT,
        _view_mode: FramedViewMode,
    ) -> Self::Output {
        EventCoordless {
            d: event.d,
            delta_t: event.delta_t,
        }
    }

    fn max_f32() -> f32 {
        1.0
    }
}

impl FrameValue for u8 {
    type Output = u8;

    fn get_frame_value(
        event: &Event,
        source_type: SourceType,
        tpf: DeltaT,
        practical_d_max: f32,
        delta_t_max: DeltaT,
        view_mode: FramedViewMode,
    ) -> Self::Output {
        match view_mode {
            FramedViewMode::Intensity => {
                let intensity = event_to_intensity(event);
                match source_type {
                    SourceType::U8 => (intensity * tpf as f64) as u8,
                    SourceType::U16 => {
                        (intensity / u16::MAX as f64 * tpf as f64 * u8::MAX as f64) as u8
                    }
                    SourceType::U32 => {
                        (intensity / u32::MAX as f64 * tpf as f64 * u8::MAX as f64) as u8
                    }
                    SourceType::U64 => {
                        (intensity / u64::MAX as f64 * tpf as f64 * u8::MAX as f64) as u8
                    }
                    SourceType::F32 => {
                        todo!()
                    }
                    SourceType::F64 => {
                        todo!()
                    }
                }
            }
            FramedViewMode::D => ((event.d as f32 / practical_d_max) * u8::MAX as f32) as u8,
            FramedViewMode::DeltaT => {
                ((event.delta_t as f32 / delta_t_max as f32) * u8::MAX as f32) as u8
            }
        }
    }

    fn max_f32() -> f32 {
        u8::MAX as f32
    }
}

impl FrameValue for u16 {
    type Output = u16;

    fn get_frame_value(
        event: &Event,
        source_type: SourceType,
        tpf: DeltaT,
        practical_d_max: f32,
        delta_t_max: DeltaT,
        view_mode: FramedViewMode,
    ) -> Self::Output {
        match view_mode {
            FramedViewMode::Intensity => {
                let intensity = event_to_intensity(event);
                match source_type {
                    SourceType::U8 => {
                        (intensity / u8::MAX as f64 * tpf as f64 * u16::MAX as f64) as u16
                    }
                    SourceType::U16 => (intensity * tpf as f64) as u16,
                    SourceType::U32 => {
                        (intensity / u32::MAX as f64 * tpf as f64 * u16::MAX as f64) as u16
                    }
                    SourceType::U64 => {
                        (intensity / u64::MAX as f64 * tpf as f64 * u16::MAX as f64) as u16
                    }
                    SourceType::F32 => {
                        todo!()
                    }
                    SourceType::F64 => {
                        todo!()
                    }
                }
            }
            FramedViewMode::D => ((event.d as f32 / practical_d_max) * u16::MAX as f32) as u16,
            FramedViewMode::DeltaT => {
                ((event.delta_t as f32 / delta_t_max as f32) * u16::MAX as f32) as u16
            }
        }
    }

    fn max_f32() -> f32 {
        u16::MAX as f32
    }
}

impl FrameValue for u32 {
    type Output = u32;

    fn get_frame_value(
        event: &Event,
        source_type: SourceType,
        tpf: DeltaT,
        practical_d_max: f32,
        delta_t_max: DeltaT,
        view_mode: FramedViewMode,
    ) -> Self::Output {
        match view_mode {
            FramedViewMode::Intensity => {
                let intensity = event_to_intensity(event);
                match source_type {
                    SourceType::U8 => {
                        (intensity / u8::MAX as f64 * tpf as f64 * u32::MAX as f64) as u32
                    }
                    SourceType::U16 => {
                        (intensity / u16::MAX as f64 * tpf as f64 * u32::MAX as f64) as u32
                    }
                    SourceType::U32 => (intensity * tpf as f64) as u32,
                    SourceType::U64 => {
                        (intensity / u64::MAX as f64 * tpf as f64 * u32::MAX as f64) as u32
                    }
                    SourceType::F32 => {
                        todo!()
                    }
                    SourceType::F64 => {
                        todo!()
                    }
                }
            }
            FramedViewMode::D => ((event.d as f32 / practical_d_max) * u32::MAX as f32) as u32,
            FramedViewMode::DeltaT => {
                ((event.delta_t as f32 / delta_t_max as f32) * u32::MAX as f32) as u32
            }
        }
    }

    fn max_f32() -> f32 {
        u32::MAX as f32
    }
}

impl FrameValue for u64 {
    type Output = u64;

    fn get_frame_value(
        event: &Event,
        source_type: SourceType,
        tpf: DeltaT,
        practical_d_max: f32,
        delta_t_max: DeltaT,
        view_mode: FramedViewMode,
    ) -> Self::Output {
        match view_mode {
            FramedViewMode::Intensity => {
                let intensity = event_to_intensity(event);
                match source_type {
                    SourceType::U8 => {
                        (intensity / u8::MAX as f64 * tpf as f64 * u64::MAX as f64) as u64
                    }
                    SourceType::U16 => {
                        (intensity / u16::MAX as f64 * tpf as f64 * u64::MAX as f64) as u64
                    }
                    SourceType::U32 => {
                        (intensity / u32::MAX as f64 * tpf as f64 * u64::MAX as f64) as u64
                    }
                    SourceType::U64 => (intensity * tpf as f64) as u64,
                    SourceType::F32 => {
                        todo!()
                    }
                    SourceType::F64 => {
                        todo!()
                    }
                }
            }
            FramedViewMode::D => ((event.d as f32 / practical_d_max) * u64::MAX as f32) as u64,
            FramedViewMode::DeltaT => {
                ((event.delta_t as f32 / delta_t_max as f32) * u64::MAX as f32) as u64
            }
        }
    }

    fn max_f32() -> f32 {
        u64::MAX as f32
    }
}

pub fn event_to_intensity(event: &Event) -> Intensity {
    match event.d as usize {
        a if a >= D_SHIFT.len() => 0 as Intensity,
        _ => match event.delta_t {
            0 => D_SHIFT[event.d as usize] as Intensity, // treat it as dt = 1
            _ => D_SHIFT[event.d as usize] as Intensity / event.delta_t as Intensity,
        },
    }
}

fn _eventcoordless_to_intensity(event: &EventCoordless) -> Intensity {
    match event.d as usize {
        a if a >= D_SHIFT.len() => 0 as Intensity,
        _ => D_SHIFT[event.d as usize] as Intensity / event.delta_t as Intensity,
    }
}
