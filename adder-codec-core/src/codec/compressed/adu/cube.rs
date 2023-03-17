use crate::codec::compressed::adu::interblock::AduInterBlock;
use crate::codec::compressed::adu::intrablock::AduIntraBlock;
use crate::codec::compressed::adu::AduCompression;
use crate::codec::compressed::stream::{CompressedInput, CompressedOutput};
use crate::codec::{ReadCompression, WriteCompression};
use bitstream_io::{BigEndian, BitReader};
use std::io::{Error, Read, Write};

pub struct AduCube {
    idx_y: u16,

    idx_x: u16,

    intra_block: AduIntraBlock,

    /// The number of inter blocks in the ADU.
    num_inter_blocks: u16,

    /// The inter blocks in the ADU.
    inter_blocks: Vec<AduInterBlock>,
}

impl AduCube {
    pub fn from_intra_block(intra_block: AduIntraBlock, idx_y: u16, idx_x: u16) -> Self {
        Self {
            idx_y,
            idx_x,
            intra_block,
            num_inter_blocks: 0,
            inter_blocks: Vec::new(),
        }
    }

    pub fn add_inter_block(&mut self, inter_block: AduInterBlock) {
        self.num_inter_blocks += 1;
        self.inter_blocks.push(inter_block);
    }
}

impl AduCompression for AduCube {
    fn compress<W: Write>(&self, output: &mut CompressedOutput<W>) -> Result<(), Error> {
        // Write the cube coordinates
        output.write_bytes(&self.idx_y.to_be_bytes())?;
        output.write_bytes(&self.idx_x.to_be_bytes())?;

        // Write the intra block
        self.intra_block.compress(output)?;

        // Write the number of inter blocks
        output.write_bytes(&self.num_inter_blocks.to_be_bytes())?;

        // Write the inter blocks
        for inter_block in &self.inter_blocks {
            inter_block.compress(output)?;
        }

        Ok(())
    }

    fn decompress<R: Read>(
        stream: &mut BitReader<R, BigEndian>,
        input: &mut CompressedInput<R>,
    ) -> Self {
        // Read the cube coordinates
        let mut bytes = [0; 2];
        input.read_bytes(&mut bytes, stream).unwrap();
        let idx_y = u16::from_be_bytes(bytes);
        input.read_bytes(&mut bytes, stream).unwrap();
        let idx_x = u16::from_be_bytes(bytes);

        // Read the intra block
        let intra_block = AduIntraBlock::decompress(stream, input);

        // Initialize empty cube
        let mut cube = Self {
            idx_y,
            idx_x,
            intra_block,
            num_inter_blocks: 0,
            inter_blocks: Vec::new(),
        };

        // Read the number of inter blocks
        input.read_bytes(&mut bytes, stream).unwrap();
        cube.num_inter_blocks = u16::from_be_bytes(bytes);

        // Read the inter blocks
        for _ in 0..cube.num_inter_blocks {
            cube.inter_blocks
                .push(AduInterBlock::decompress(stream, input));
        }

        cube
    }
}
