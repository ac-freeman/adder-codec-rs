use crate::codec::compressed::adu::interblock::AduInterBlock;
use crate::codec::compressed::adu::intrablock::AduIntraBlock;

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

    fn compress() -> Vec<u8> {
        todo!()
    }

    fn decompress() -> Self {
        todo!()
    }
}
