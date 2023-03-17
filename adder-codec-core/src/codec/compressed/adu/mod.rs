use crate::codec::compressed::stream::{CompressedInput, CompressedOutput};
use bitstream_io::{BigEndian, BitReader};
use std::io::{Read, Write};

pub mod cube;
pub mod frame;
pub mod interblock;
pub mod intrablock;

trait AduCompression {
    fn compress<W: Write>(&self, output: &mut CompressedOutput<W>) -> Result<(), std::io::Error>;
    fn decompress<R: Read>(
        stream: &mut BitReader<R, BigEndian>,
        input: &mut CompressedInput<R>,
    ) -> Self;
}
