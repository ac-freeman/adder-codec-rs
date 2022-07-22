use crate::{BigT, Intensity};

pub trait ScaleIntensity <T> {
    fn scale_intensity(intensity: Intensity, tpf: BigT) -> T;
}

/// Scales the event's intensity for a u8 source to a u16 output
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

/// Scales the event's intensity for a u8 source to a u32 output
impl ScaleIntensity<u64> for u8 {
    fn scale_intensity(intensity: Intensity, tpf: BigT) -> u64 {
        (intensity / u8::MAX as f64 * tpf as f64 * u64::MAX as f64) as u64
    }
}

/// Scales the event's intensity for a u8 source to a u16 output
impl ScaleIntensity<u8> for u16 {
    fn scale_intensity(intensity: Intensity, tpf: BigT) -> u8 {
        (intensity / u16::MAX as f64 * tpf as f64 * u8::MAX as f64) as u8
    }
}

/// Scales the event's intensity for a u8 source to a u16 output
impl ScaleIntensity<u16> for u16 {
    fn scale_intensity(intensity: Intensity, tpf: BigT) -> u16 {
        (intensity * tpf as f64) as u16
    }
}

/// Scales the event's intensity for a u8 source to a u32 output
impl ScaleIntensity<u32> for u16 {
    fn scale_intensity(intensity: Intensity, tpf: BigT) -> u32 {
        (intensity / u16::MAX as f64 * tpf as f64 * u32::MAX as f64) as u32
    }
}

/// Scales the event's intensity for a u8 source to a u32 output
impl ScaleIntensity<u64> for u16 {
    fn scale_intensity(intensity: Intensity, tpf: BigT) -> u64 {
        (intensity / u16::MAX as f64 * tpf as f64 * u64::MAX as f64) as u64
    }
}


/// Scales the event's intensity for a u8 source to a u16 output
impl ScaleIntensity<u8> for u32 {
    fn scale_intensity(intensity: Intensity, tpf: BigT) -> u8 {
        (intensity / u32::MAX as f64 * tpf as f64 * u8::MAX as f64) as u8
    }
}

/// Scales the event's intensity for a u8 source to a u16 output
impl ScaleIntensity<u16> for u32 {
    fn scale_intensity(intensity: Intensity, tpf: BigT) -> u16 {
        (intensity / u32::MAX as f64 * tpf as f64 * u16::MAX as f64) as u16
    }
}

/// Scales the event's intensity for a u8 source to a u32 output
impl ScaleIntensity<u32> for u32 {
    fn scale_intensity(intensity: Intensity, tpf: BigT) -> u32 {
        (intensity * tpf as f64) as u32
    }
}

/// Scales the event's intensity for a u8 source to a u32 output
impl ScaleIntensity<u64> for u32 {
    fn scale_intensity(intensity: Intensity, tpf: BigT) -> u64 {
        (intensity / u32::MAX as f64 * tpf as f64 * u64::MAX as f64) as u64
    }
}

/// Scales the event's intensity for a u8 source to a u16 output
impl ScaleIntensity<u8> for u64 {
    fn scale_intensity(intensity: Intensity, tpf: BigT) -> u8 {
        (intensity / u64::MAX as f64 * tpf as f64 * u8::MAX as f64) as u8
    }
}

/// Scales the event's intensity for a u8 source to a u16 output
impl ScaleIntensity<u16> for u64 {
    fn scale_intensity(intensity: Intensity, tpf: BigT) -> u16 {
        (intensity / u64::MAX as f64 * tpf as f64 * u16::MAX as f64) as u16
    }
}

/// Scales the event's intensity for a u8 source to a u32 output
impl ScaleIntensity<u32> for u64 {
    fn scale_intensity(intensity: Intensity, tpf: BigT) -> u32 {
        (intensity / u64::MAX as f64 * tpf as f64 * u32::MAX as f64) as u32
    }
}

/// Scales the event's intensity for a u8 source to a u32 output
impl ScaleIntensity<u64> for u64 {
    fn scale_intensity(intensity: Intensity, tpf: BigT) -> u64 {
        (intensity * tpf as f64) as u64
    }
}