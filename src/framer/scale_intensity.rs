use crate::{BigT, D_SHIFT, DeltaT, Event, EventCoordless, Intensity};
use crate::framer::framer::SourceType;

pub trait ScaleIntensity <T> {
    fn scale_intensity(intensity: Intensity, tpf: BigT) -> T;
}

/// Scales the event's intensity for a u8 source to a u8 output
impl ScaleIntensity<u8> for u8 {
    fn scale_intensity(intensity: Intensity, tpf: BigT) -> u8 {
        (intensity * tpf as f64) as u8
    }
}

/// Scales the event's intensity for a u8 source to a u16 output
impl ScaleIntensity<u16> for u8 {
    fn scale_intensity(intensity: Intensity, tpf: BigT) -> u16 {
        (intensity / u8::MAX as f64 * tpf as f64 * u16::MAX as f64) as u16
    }
}

/// Scales the event's intensity for a u8 source to a u32 output
impl ScaleIntensity<u32> for u8 {
    fn scale_intensity(intensity: Intensity, tpf: BigT) -> u32 {
        (intensity / u8::MAX as f64 * tpf as f64 * u32::MAX as f64) as u32
    }
}

/// Scales the event's intensity for a u8 source to a u64 output
impl ScaleIntensity<u64> for u8 {
    fn scale_intensity(intensity: Intensity, tpf: BigT) -> u64 {
        (intensity / u8::MAX as f64 * tpf as f64 * u64::MAX as f64) as u64
    }
}

/// Scales the event's intensity for a u16 source to a u8 output
impl ScaleIntensity<u8> for u16 {
    fn scale_intensity(intensity: Intensity, tpf: BigT) -> u8 {
        (intensity / u16::MAX as f64 * tpf as f64 * u8::MAX as f64) as u8
    }
}

/// Scales the event's intensity for a u16 source to a u16 output
impl ScaleIntensity<u16> for u16 {
    fn scale_intensity(intensity: Intensity, tpf: BigT) -> u16 {
        (intensity * tpf as f64) as u16
    }
}

/// Scales the event's intensity for a u16 source to a u32 output
impl ScaleIntensity<u32> for u16 {
    fn scale_intensity(intensity: Intensity, tpf: BigT) -> u32 {
        (intensity / u16::MAX as f64 * tpf as f64 * u32::MAX as f64) as u32
    }
}

/// Scales the event's intensity for a u16 source to a u64 output
impl ScaleIntensity<u64> for u16 {
    fn scale_intensity(intensity: Intensity, tpf: BigT) -> u64 {
        (intensity / u16::MAX as f64 * tpf as f64 * u64::MAX as f64) as u64
    }
}


/// Scales the event's intensity for a u32 source to a u8 output
impl ScaleIntensity<u8> for u32 {
    fn scale_intensity(intensity: Intensity, tpf: BigT) -> u8 {
        (intensity / u32::MAX as f64 * tpf as f64 * u8::MAX as f64) as u8
    }
}

/// Scales the event's intensity for a u32 source to a u16 output
impl ScaleIntensity<u16> for u32 {
    fn scale_intensity(intensity: Intensity, tpf: BigT) -> u16 {
        (intensity / u32::MAX as f64 * tpf as f64 * u16::MAX as f64) as u16
    }
}

/// Scales the event's intensity for a u32 source to a u32 output
impl ScaleIntensity<u32> for u32 {
    fn scale_intensity(intensity: Intensity, tpf: BigT) -> u32 {
        (intensity * tpf as f64) as u32
    }
}

/// Scales the event's intensity for a u32 source to a u64 output
impl ScaleIntensity<u64> for u32 {
    fn scale_intensity(intensity: Intensity, tpf: BigT) -> u64 {
        (intensity / u32::MAX as f64 * tpf as f64 * u64::MAX as f64) as u64
    }
}

/// Scales the event's intensity for a u64 source to a u8 output
impl ScaleIntensity<u8> for u64 {
    fn scale_intensity(intensity: Intensity, tpf: BigT) -> u8 {
        (intensity / u64::MAX as f64 * tpf as f64 * u8::MAX as f64) as u8
    }
}

/// Scales the event's intensity for a u64 source to a u16 output
impl ScaleIntensity<u16> for u64 {
    fn scale_intensity(intensity: Intensity, tpf: BigT) -> u16 {
        (intensity / u64::MAX as f64 * tpf as f64 * u16::MAX as f64) as u16
    }
}

/// Scales the event's intensity for a u64 source to a u32 output
impl ScaleIntensity<u32> for u64 {
    fn scale_intensity(intensity: Intensity, tpf: BigT) -> u32 {
        (intensity / u64::MAX as f64 * tpf as f64 * u32::MAX as f64) as u32
    }
}

/// Scales the event's intensity for a u64 source to a u64 output
impl ScaleIntensity<u64> for u64 {
    fn scale_intensity(intensity: Intensity, tpf: BigT) -> u64 {
        (intensity * tpf as f64) as u64
    }
}


pub(crate) trait FrameValue {
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