use arithmetic_coding_adder_dep::{Decoder, Encoder, Model};
use bitstream_io::{BigEndian, BitReader, BitWrite, BitWriter};

pub fn round_trip<M>(model: M, input: &[M::Symbol])
where
    M: Model + Clone,
    M::Symbol: PartialEq + std::fmt::Debug + Clone,
{
    let buffer = encode(model.clone(), input.to_owned());
    let output = decode(model, &buffer);

    assert_eq!(input, output.as_slice());
}

fn encode<M>(model: M, input: Vec<M::Symbol>) -> Vec<u8>
where
    M: Model,
{
    let mut bitwriter = BitWriter::endian(Vec::new(), BigEndian);
    let mut encoder = Encoder::new(model);

    encoder.encode_all(input, &mut bitwriter).unwrap();
    bitwriter.byte_align().unwrap();

    bitwriter.into_writer()
}

fn decode<M>(model: M, buffer: &[u8]) -> Vec<M::Symbol>
where
    M: Model,
{
    let mut bitreader = BitReader::endian(buffer, BigEndian);
    let mut decoder = Decoder::new(model);

    decoder
        .decode_all(&mut bitreader)
        .map(Result::unwrap)
        .collect()
}
