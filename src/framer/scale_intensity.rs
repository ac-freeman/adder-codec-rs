use crate::{BigT, D_SHIFT, DeltaT, Event, EventCoordless, Intensity};
use crate::framer::event_framer::SourceType;

pub trait FrameValue {
    type Output;
    fn get_frame_value(event: &Event, source_type: SourceType, i1: DeltaT) -> Self::Output;
}

impl FrameValue for EventCoordless {
    type Output = EventCoordless;
    fn get_frame_value(event: &Event, _source_type: SourceType, _tpf: DeltaT) -> Self::Output {
        EventCoordless { d: event.d, delta_t: event.delta_t }
    }
}

impl FrameValue for u8 {
    type Output = u8;

    fn get_frame_value(event: &Event, source_type: SourceType, tpf: DeltaT) -> Self::Output {
        let intensity = event_to_intensity(event);
        match source_type {
            SourceType::U8 => { (intensity * tpf as f64) as u8 }
            SourceType::U16 => { (intensity / u16::MAX as f64 * tpf as f64 * u8::MAX as f64) as u8 }
            SourceType::U32 => { (intensity / u32::MAX as f64 * tpf as f64 * u8::MAX as f64) as u8 }
            SourceType::U64 => { (intensity / u64::MAX as f64 * tpf as f64 * u8::MAX as f64) as u8 }
            SourceType::F32 => { todo!() }
            SourceType::F64 => { todo!() }
        }
    }
}

impl FrameValue for u16 {
    type Output = u16;

    fn get_frame_value(event: &Event, source_type: SourceType, tpf: DeltaT) -> Self::Output {
        let intensity = event_to_intensity(event);
        match source_type {
            SourceType::U8 => { (intensity / u8::MAX as f64 * tpf as f64 * u16::MAX as f64) as u16 }
            SourceType::U16 => { (intensity * tpf as f64) as u16 }
            SourceType::U32 => { (intensity / u32::MAX as f64 * tpf as f64 * u16::MAX as f64) as u16 }
            SourceType::U64 => { (intensity / u64::MAX as f64 * tpf as f64 * u16::MAX as f64) as u16 }
            SourceType::F32 => { todo!() }
            SourceType::F64 => { todo!() }
        }
    }
}

impl FrameValue for u32 {
    type Output = u32;

    fn get_frame_value(event: &Event, source_type: SourceType, tpf: DeltaT) -> Self::Output {
        let intensity = event_to_intensity(event);
        match source_type {
            SourceType::U8 => { (intensity / u8::MAX as f64 * tpf as f64 * u32::MAX as f64) as u32 }
            SourceType::U16 => { (intensity / u16::MAX as f64 * tpf as f64 * u32::MAX as f64) as u32 }
            SourceType::U32 => { (intensity * tpf as f64) as u32 }
            SourceType::U64 => { (intensity / u64::MAX as f64 * tpf as f64 * u32::MAX as f64) as u32 }
            SourceType::F32 => { todo!() }
            SourceType::F64 => { todo!() }
        }
    }
}

impl FrameValue for u64 {
    type Output = u64;

    fn get_frame_value(event: &Event, source_type: SourceType, tpf: DeltaT) -> Self::Output {
        let intensity = event_to_intensity(event);
        match source_type {
            SourceType::U8 => { (intensity / u8::MAX as f64 * tpf as f64 * u64::MAX as f64) as u64 }
            SourceType::U16 => { (intensity / u16::MAX as f64 * tpf as f64 * u64::MAX as f64) as u64 }
            SourceType::U32 => { (intensity / u32::MAX as f64 * tpf as f64 * u64::MAX as f64) as u64 }
            SourceType::U64 => { (intensity * tpf as f64) as u64 }
            SourceType::F32 => { todo!() }
            SourceType::F64 => { todo!() }
        }
    }
}


fn event_to_intensity(event: &Event) -> Intensity {
    match event.d as usize {
        a if a >= D_SHIFT.len() => {
            0 as Intensity
        },
        _ => {
            D_SHIFT[event.d as usize] as Intensity / event.delta_t as Intensity
        }
    }

}