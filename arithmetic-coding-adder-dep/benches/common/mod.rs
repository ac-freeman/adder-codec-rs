use arithmetic_coding_adder_dep::{Model};


pub fn round_trip<M>(model: M, input: &[M::Symbol])
where
    M: Model + Clone,
    M::Symbol: Copy + std::fmt::Debug + PartialEq,
{
    let buffer = encode(model.clone(), input.iter().copied());

    let mut output = Vec::with_capacity(input.len());
    for symbol in decode(model, &buffer) {
        output.push(symbol);
    }

    assert_eq!(input, output.as_slice());
}

pub fn encode<M, I>(_model: M, _input: I) -> Vec<u8>
where
    M: Model,
    I: IntoIterator<Item = M::Symbol>,
{
    todo!()
    // let mut bitwriter = BitWriter::endian(Vec::new(), BigEndian);
    // let mut encoder = Encoder::new(model);
    //
    // encoder.encode_all(input).unwrap();
    // bitwriter.byte_align().unwrap();
    //
    // bitwriter.into_writer()
}

pub fn decode<M>(_model: M, _buffer: &[u8]) -> Vec<M::Symbol>
where
    M: Model,
{
    todo!()
    // let bitreader = BitReader::endian(buffer, BigEndian);
    // let mut decoder = Decoder::new(model, bitreader);
    // decoder.decode_all().map(Result::unwrap).collect()
}
