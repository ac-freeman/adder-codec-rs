use arithmetic_coding_adder_dep::{Decoder, Encoder, Model};
use bitstream_io::{BigEndian, BitReader, BitWrite, BitWriter};

#[allow(unused)]
pub fn round_trip<M>(model: M, input: Vec<M::Symbol>)
where
    M: Model + Clone,
    M::Symbol: std::fmt::Debug,
{
    println!("input: {:?}", &input);

    println!("\nencoding...");
    let buffer = encode(model.clone(), input);

    println!("buffer: {:?}", &buffer);

    println!("\ndecoding...");
    for symbol in decode(model, &buffer) {
        println!("{symbol:?}");
    }
}

pub fn encode<M, I>(model: M, input: I) -> Vec<u8>
where
    M: Model,
    I: IntoIterator<Item = M::Symbol>,
{
    let mut bitwriter = BitWriter::endian(Vec::new(), BigEndian);
    let mut encoder = Encoder::new(model);

    encoder.encode_all(input).unwrap();
    bitwriter.byte_align().unwrap();

    bitwriter.into_writer()
}

pub fn decode<M>(model: M, buffer: &[u8]) -> Vec<M::Symbol>
where
    M: Model,
{
    let bitreader = BitReader::endian(buffer, BigEndian);
    let mut decoder = Decoder::new(model, bitreader);
    decoder.decode_all().map(Result::unwrap).collect()
}

#[allow(unused)]
pub fn round_trip_string<M>(model: M, input: &str)
where
    M: Model<Symbol = char> + Clone,
{
    let input_bytes = input.bytes().len();

    let buffer = encode(model.clone(), input.chars());

    let output_bytes = buffer.len();

    println!("input bytes: {input_bytes}");
    println!("output bytes: {output_bytes}");

    println!(
        "compression ratio: {}",
        input_bytes as f32 / output_bytes as f32
    );

    let output = decode(model, &buffer);

    let mut prefix: String = output.into_iter().take(299).collect();
    prefix.push_str("...");

    println!("{prefix}");
}
