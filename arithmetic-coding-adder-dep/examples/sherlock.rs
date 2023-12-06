use std::{fs::File, io::Read, ops::Range};

use arithmetic_coding_adder_dep::Model;

mod common;

const ALPHABET: &str =
    "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789 .,\n-':()[]#*;\"!?*&é/àâè%@$";

#[derive(Debug, Clone)]
pub struct StringModel {
    alphabet: Vec<char>,
}

impl StringModel {
    #[must_use]
    pub fn new(alphabet: Vec<char>) -> Self {
        Self { alphabet }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("invalid character: {0}")]
pub struct Error(char);

impl Model for StringModel {
    type Symbol = char;
    type ValueError = Error;

    fn probability(&self, symbol: Option<&Self::Symbol>) -> Result<Range<u32>, Error> {
        if let Some(char) = symbol {
            match self.alphabet.iter().position(|x| x == char) {
                Some(index) => Ok((index as u32)..(index as u32 + 1)),
                None => Err(Error(*char)),
            }
        } else {
            let alphabet_length = self.alphabet.len() as u32;
            Ok(alphabet_length..(alphabet_length + 1))
        }
    }

    fn symbol(&self, value: u32) -> Option<Self::Symbol> {
        self.alphabet.get(value as usize).copied()
    }

    fn max_denominator(&self) -> u32 {
        self.alphabet.len() as u32 + 1
    }
}

fn main() {
    let mut file = File::open("./resources/sherlock.txt").unwrap();
    let mut input = String::new();
    file.read_to_string(&mut input).unwrap();
    let input_bytes = input.bytes().len();

    let model = StringModel::new(ALPHABET.chars().collect());

    let buffer = common::encode(model.clone(), input.chars());

    let output_bytes = buffer.len();

    println!("input bytes: {input_bytes}");
    println!("output bytes: {output_bytes}");

    println!(
        "compression ratio: {}",
        input_bytes as f32 / output_bytes as f32
    );

    // println!("buffer: {:?}", &buffer);

    let output = common::decode(model, &buffer);

    let mut prefix: String = output.into_iter().take(299).collect();
    prefix.push_str("...");

    println!("{prefix}");
}
