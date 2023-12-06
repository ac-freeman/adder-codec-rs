use crate::utils::logging::LogFeatureSource::OpenCV;
use adder_codec_core::Coord;
use opencv::core::KeyPoint;
use serde::ser::SerializeStruct;
use serde::{Serialize, Serializer};

#[derive(Debug, Clone, Copy)]
pub struct LogFeature {
    pub x: u16,
    pub y: u16,
    pub non_max_suppression: bool,
    pub source: LogFeatureSource,
}

impl Serialize for LogFeature {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("LogFeature", 4)?;
        state.serialize_field("x", &self.x)?;
        state.serialize_field("y", &self.y)?;
        state.serialize_field("n", &self.non_max_suppression)?;
        state.serialize_field("s", &self.source)?;
        state.end()
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
pub enum LogFeatureSource {
    ADDER,
    OpenCV,
}

impl LogFeature {
    pub fn from_coord(coord: Coord, source: LogFeatureSource, non_max_suppression: bool) -> Self {
        Self {
            x: coord.x,
            y: coord.y,
            non_max_suppression,
            source,
        }
    }

    pub fn from_keypoint(
        keypoint: &KeyPoint,
        source: LogFeatureSource,
        non_max_suppression: bool,
    ) -> Self {
        let mut v: opencv::core::Vector<KeyPoint> = opencv::core::Vector::new();
        v.push(keypoint.clone());

        let mut points: opencv::core::Vector<opencv::core::Point2f> = opencv::core::Vector::new();
        let mut indices: opencv::core::Vector<i32> = opencv::core::Vector::new();
        indices.push(0);
        KeyPoint::convert(&v, &mut points, &indices).unwrap();

        let p = points.get(0).unwrap();
        Self {
            x: p.x as u16,
            y: p.y as u16,
            non_max_suppression,
            source,
        }
    }
}
