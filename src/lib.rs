use std::fs::File;
use std::io::BufWriter;
use std::path::Path;
use bytes::Bytes;
use crate::header::EventStreamHeader;

mod header;
pub mod raw;

/// Decimation value; a pixel's sensitivity.
pub type D = u8;

type Integration = f32;

/// Number of ticks elapsed since a given pixel last fired an [`pixel::Event`]
pub type DeltaT = u32;

/// Measure of an amount of light intensity
pub type Intensity = f32;

/// Pixel x- or y- coordinate address in the ADΔER model
pub type PixelAddress = u16;

#[derive(Debug, Copy, Clone)]
pub struct Coord {
    pub(crate) x: PixelAddress,
    pub(crate) y: PixelAddress,
    pub(crate) c: Option<u8>,
}

/// An ADΔER event representation
#[derive(Debug, Copy, Clone)]
pub struct Event {
    pub(crate) coord: Coord,
    pub(crate) d: D,
    pub(crate) delta_t: DeltaT,
}

impl From<&Coord> for Bytes {
    fn from(coord: &Coord) -> Self {
        match coord.c {
            None => {
                Bytes::from([
                    &coord.x.to_be_bytes() as &[u8],
                    &coord.y.to_be_bytes() as &[u8],
                ].concat()
                )
            }
            Some(c) => {
                Bytes::from([
                    &coord.x.to_be_bytes() as &[u8],
                    &coord.y.to_be_bytes() as &[u8],
                    &c.to_be_bytes() as &[u8]
                ].concat()
                )
            }
        }

    }
}

impl From<&Event> for Bytes {
    fn from(event: &Event) -> Self {
        Bytes::from([
            &Bytes::from(&event.coord).to_vec() as &[u8],
            &event.d.to_be_bytes() as &[u8],
            &event.delta_t.to_be_bytes() as &[u8]
        ].concat()
        )
    }
}

pub trait Codec {
    fn new() -> Self;

    fn open_writer<P: AsRef<Path>>(&mut self, path: P) -> Result<(), std::io::Error>{
        let file = File::create(&path)?;
        self.set_output_stream(Some(BufWriter::new(file)));
        Ok(())
    }

    fn open_reader<P: AsRef<Path>>(&mut self, path: P) -> Result<(), std::io::Error>{
        let file = File::create(&path)?;
        self.set_input_stream(Some(BufWriter::new(file)));
        Ok(())
    }
    fn set_output_stream(&mut self, stream: Option<BufWriter<File>>);
    fn set_input_stream(&mut self, stream: Option<BufWriter<File>>);

    fn serialize_header(&mut self,
                        width: u16,
                        height: u16,
                        tps: u32,
                        ref_interval: u32,
                        delta_t_max: u32,
                        channels: u8);
    fn encode_event(&mut self, event: &Event);
}






#[cfg(test)]
mod tests {
    // use crate::EventStreamHeader;
    // use crate::header::MAGIC_RAW;

    #[test]
    fn encode_raw() {

    }
}
