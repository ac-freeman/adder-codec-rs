use arithmetic_coding::{Decoder, Encoder, Model};
use bitstream_io::{BigEndian, BitReader, BitWrite, BitWriter};

pub fn round_trip<M>(model: M, input: Vec<M::Symbol>)
where
    M: Model + Clone,
    M::Symbol: PartialEq + std::fmt::Debug + Clone,
{
    let buffer = encode(model.clone(), input.clone());
    let output = decode(model, &buffer);

    assert_eq!(input, output);
}

fn encode<M>(model: M, input: Vec<M::Symbol>) -> Vec<u8>
where
    M: Model,
{
    let mut bitwriter = BitWriter::endian(Vec::new(), BigEndian);
    let mut encoder = Encoder::<M>::new(model);

    encoder.encode_all(input, &mut bitwriter).expect("failed to encode data!");
    bitwriter.byte_align().expect("failed to byte-align the stream");

    bitwriter.into_writer()
}

fn decode<M>(model: M, buffer: &[u8]) -> Vec<M::Symbol>
where
    M: Model,
{
    let bitreader = BitReader::endian(buffer, BigEndian);
    let mut decoder = Decoder::new(model, bitreader).unwrap();

    decoder.decode_all().map(Result::unwrap).collect()
}
