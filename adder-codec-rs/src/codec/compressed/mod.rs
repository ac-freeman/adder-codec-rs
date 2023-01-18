/// Sketch of idea for compressed AVU format
///
/// At the beginning, spatially divide the frame from the ground up into blocks based on D values of
/// the first event for each pixel. Smallest block size is a single pixel (1x1). Largest block size
/// for now is 64x64. Just use square blocks for now, for simplicity.
///
/// Intra coding:
///
/// When building the tree, look at the first 2x2 block. If all 4 pixels have the same D value, then
/// we output a bit 1, otherwise 0. Proceed until have done 4x4 block. AND the bits together to get
/// the 4x4 block status. If all 16 pixels have the same D value, then we output a bit 1, otherwise
/// 0 1110, for example, to indicate the bottom right 2x2 block is different from the rest.
///
/// Suppose video is 128x128 pixels, and the tree is described with bits 0 1110 1010 1110 1110 1101
/// 0111 0000 1101
/// Then the BR 64x64 block doesn't have uniform D. Within that 64x64 block, the TR and BR 32x32
/// blocks don't have uniform D (1010). We first look at the TR block, and see that its BR 16x16
/// block doesn't have uniform D (1110). We then look at the BR 16x16 block, and see that its BR 8x8
/// block doesn't have uniform D (1110). We then look at the BR 8x8 block, and see that its BL 4x4
/// block doesn't have uniform D (1101). We then look at the BL 4x4 block, and see that its TL 2x2
/// block doesn't have uniform D (0111). We then look at the TL 2x2 block, and see that all of its
/// pixels have different D values (0000). We then bubble up to 64x64 block and look at the BR block
/// (1101)... and so on.
///
fn void() {}
