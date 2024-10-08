extern crate adder_codec_core;

use adder_codec_core::codec::compressed::stream::CompressedOutput;
use adder_codec_core::codec::encoder::Encoder;

use adder_codec_core::codec::{CodecError, EncoderOptions};
use adder_codec_core::open_file_decoder;
use std::error::Error;
use std::io::BufWriter;

#[test]
fn test_read_adder_raw() -> Result<(), Box<dyn Error>> {
    // Open the virat_small_gray.adder sample file as a RawInput
    let (stream, _bitreader) = open_file_decoder("tests/samples/virat_small_gray.adder")?;

    assert!(stream.meta().plane.w() == 192);

    Ok(())
}

#[test]
fn test_build_first_frame() -> Result<(), Box<dyn Error>> {
    // Open the virat_small_gray.adder sample file as a RawInput
    let (mut stream, mut bitreader) = open_file_decoder("tests/samples/virat_small_gray.adder")?;
    stream.meta_mut().adu_interval =
        (stream.meta().delta_t_max / stream.meta().ref_interval) as usize; // This is a fix since we're reading a v2-encoded file

    // Create the compressed encoder
    let bufwriter = BufWriter::new(vec![]);
    let compression = CompressedOutput::new(*stream.meta(), bufwriter);

    let mut encoder: Encoder<BufWriter<Vec<u8>>> =
        Encoder::new_compressed(compression, EncoderOptions::default((stream.meta()).plane));

    for _i in 0..24000 {
        // Loop through the events and ingest them to the compressor
        let event = stream.digest_event(&mut bitreader)?;
        encoder.ingest_event(event)?;
    }

    Ok(())
}

#[test]
fn test_build_many_frames() -> Result<(), Box<dyn Error>> {
    // Open the virat_small_gray.adder sample file as a RawInput
    let (mut stream, mut bitreader) = open_file_decoder("tests/samples/virat_small_gray.adder")?;
    stream.meta_mut().adu_interval =
        (stream.meta().delta_t_max / stream.meta().ref_interval) as usize; // This is a fix since we're reading a v2-encoded file

    // Create the compressed encoder
    let bufwriter = BufWriter::new(vec![]);
    let compression = CompressedOutput::new(*stream.meta(), bufwriter);
    let mut encoder: Encoder<BufWriter<Vec<u8>>> =
        Encoder::new_compressed(compression, EncoderOptions::default((stream.meta()).plane));

    let mut event_count: u32 = 0;
    loop {
        // Loop through the events and ingest them to the compressor
        let res = stream.digest_event(&mut bitreader);
        match res {
            Ok(event) => {
                encoder.ingest_event(event)?;
                event_count += 1;
            }
            Err(CodecError::IoError(_e)) => {
                break;
            }
            Err(e) => return Err(Box::new(e)),
        }
    }

    encoder.flush_writer()?;
    let writer = encoder.close_writer()?.unwrap();
    let compressed = writer.into_inner()?;
    dbg!(compressed.len());
    assert!((compressed.len() as u32) < event_count * stream.meta().event_size as u32);

    Ok(())
}
