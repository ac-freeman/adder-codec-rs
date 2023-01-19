use crate::framer::driver::EventCoordless;
use crate::{DeltaT, Event, D};
use bitvec::prelude::*;
use bitvec::slice::Iter;
use ndarray::Array2;
use std::iter::Enumerate;

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
fn void() {
    let mut bv = bitvec![u8, Msb0;];
}

const BLOCK_SIZE: usize = 64;

pub type Block = [Option<EventCoordless>; BLOCK_SIZE * BLOCK_SIZE];

#[derive(Default, PartialEq, Debug)]
struct Cube {
    pub(crate) a: Option<Box<Cube>>,
    pub(crate) b: Option<Box<Cube>>,
    pub(crate) c: Option<Box<Cube>>,
    pub(crate) d: Option<Box<Cube>>,
    d_val: D,
    t: DeltaT,
}

#[derive(Default)]
struct CubeHead {
    cube: Cube,
    tree: BitVec<u8, Msb0>,
}

impl CubeHead {
    // fn new() -> Self {
    //     Self::default()
    // }
    //
    // fn new_from_block(&self, block: &Block) -> Self {
    //     let mut cube = Cube::default();
    //
    //     let mut bv_2_2 = bitvec![u8, Msb0;];
    //
    //     // Traverse the block and build the tree
    //     // Top left pixel at each sub-block is the reference pixel
    //     for y in (0..BLOCK_SIZE).step_by(2) {
    //         for x in (0..BLOCK_SIZE).step_by(2) {
    //             let a = block[raw_block_idx(y, x)];
    //             let b = block[raw_block_idx(y, x + 1)];
    //             let c = block[raw_block_idx(y + 1, x)];
    //             let d = block[raw_block_idx(y + 1, x + 1)];
    //             bv_2_2.push(a.d == b.d);
    //             bv_2_2.push(a.d == c.d);
    //             bv_2_2.push(a.d == d.d);
    //         }
    //     }
    //
    //     let mut bv_4_4 = bitvec![u8, Msb0;];
    //     let mut iter = bv_2_2.iter();
    //     for i in 0..(BLOCK_SIZE / 2) * (BLOCK_SIZE / 2) {
    //         let b = iter.next().unwrap();
    //         let c = iter.next().unwrap();
    //         let d = iter.next().unwrap();
    //
    //         match *b && *c && *d {
    //             true => {
    //                 bv_4_4.push(true);
    //             }
    //             false => {
    //                 bv_4_4.push(false);
    //                 bv_4_4.push(*b);
    //                 bv_4_4.push(*c);
    //                 bv_4_4.push(*d);
    //             }
    //         }
    //     }
    //
    //     // let mut bv_8_8 = bitvec![u8, Msb0;];
    //     // let mut iter = bv_4_4.iter();
    //     // for i in 0..(BLOCK_SIZE / 4) * (BLOCK_SIZE / 4) {
    //     //     let b = iter.next().unwrap();
    //     //     let c = iter.next().unwrap();
    //     //     let d = iter.next().unwrap();
    //     //
    //     //     match b && c && *d {
    //     //         true => {
    //     //             bv_8_8.push(true);
    //     //         }
    //     //         false => {
    //     //             bv_8_8.push(false);
    //     //
    //     //             if *b {
    //     //                 bv_8_8.push(true);
    //     //             } else {
    //     //                 bv_8_8.push(false);
    //     //                 bv_8_8.push(iter.next().unwrap());
    //     //                 bv_8_8.push(iter.next().unwrap());
    //     //                 bv_8_8.push(iter.next().unwrap());
    //     //             }
    //     //
    //     //             bv_8_8.push(*b);
    //     //             bv_8_8.push(*c);
    //     //             bv_8_8.push(*d);
    //     //         }
    //     //     }
    //     // }
    //
    //     // cube.d_val = block.d_val;
    //     // cube.t = block.t;
    //     Self {
    //         cube,
    //         tree: BitVec::new(),
    //     }
    // }
}

impl Cube {
    fn decode_from_tree(mut iter: &mut Iter<u8, Msb0>, level: usize) -> Self {
        // let mut iter = bv.iter();
        let mut cube = Cube::default();

        let mut iter_clone = iter.clone();

        println!("Level {}", level);
        loop {
            match iter_clone.next() {
                Some(bit) => {
                    print!("{} ", bit);
                }
                None => {
                    break;
                }
            }
        }
        println!("");
        // panic!();

        let a = iter.next().unwrap();
        let b = iter.next().unwrap();
        let c = iter.next().unwrap();
        let d = iter.next().unwrap();
        println!("{} {} {} {}", a, b, c, d);

        cube.a = Some(Box::new(Cube::default()));
        if !a {
            if level > 4 {
                println!("Following a");
                cube.a = Some(Box::new(Self::decode_from_tree(iter, level / 2)));
            }
        }

        cube.b = Some(Box::new(Cube::default()));
        if !b {
            if level > 4 {
                println!("Following b");
                cube.b = Some(Box::new(Self::decode_from_tree(iter, level / 2)));
            }
        }

        cube.c = Some(Box::new(Cube::default()));
        if !c {
            if level > 4 {
                println!("Following c");
                cube.c = Some(Box::new(Self::decode_from_tree(iter, level / 2)));
            }
        }

        cube.d = Some(Box::new(Cube::default()));
        if !d {
            if level > 4 {
                println!("Following d");
                cube.d = Some(Box::new(Self::decode_from_tree(iter, level / 2)));
            }
        }

        cube
    }
}

// start with level = 64
fn encode_block(
    mut iter: &mut Iter<u8, Msb0>,
    block: &Block,
    level: usize,
    mut x_offset: usize,
    mut y_offset: usize,
    mut output: &mut BitVec<u8, Msb0>,
    mut output_test: &mut Vec<DeltaT>,
) {
    let a = iter.next().unwrap();
    let b = iter.next().unwrap();
    let c = iter.next().unwrap();
    let d = iter.next().unwrap();

    if level == 2 {
        for y in y_offset..y_offset + (level) {
            for x in x_offset..x_offset + (level) {
                output.append(&mut BitVec::<u8, Msb0>::from_slice(
                    &block[raw_block_idx(y, x)].unwrap().d.to_be_bytes(),
                ));
                output.append(&mut BitVec::<u8, Msb0>::from_slice(
                    &block[raw_block_idx(y, x)].unwrap().delta_t.to_be_bytes(),
                ));
                output_test.push(block[raw_block_idx(y, x)].unwrap().delta_t);
            }
        }
        return;
    }

    if *a {
        output.append(&mut BitVec::<u8, Msb0>::from_slice(
            &block[raw_block_idx(y_offset, x_offset)]
                .unwrap()
                .d
                .to_be_bytes(),
        ));
        for y in y_offset..y_offset + (level / 2) {
            for x in x_offset..x_offset + (level / 2) {
                output.append(&mut BitVec::<u8, Msb0>::from_slice(
                    &block[raw_block_idx(y, x)].unwrap().delta_t.to_be_bytes(),
                ));
                output_test.push(block[raw_block_idx(y, x)].unwrap().delta_t);
            }
        }
    } else {
        encode_block(
            iter,
            block,
            level / 2,
            x_offset,
            y_offset,
            &mut output,
            &mut output_test,
        );
    }
    x_offset += (level / 2);
    if *b {
        output.append(&mut BitVec::<u8, Msb0>::from_slice(
            &block[raw_block_idx(y_offset, x_offset)]
                .unwrap()
                .d
                .to_be_bytes(),
        ));
        for y in y_offset..y_offset + (level / 2) {
            for x in x_offset..x_offset + (level / 2) {
                output.append(&mut BitVec::<u8, Msb0>::from_slice(
                    &block[raw_block_idx(y, x)].unwrap().delta_t.to_be_bytes(),
                ));
                output_test.push(block[raw_block_idx(y, x)].unwrap().delta_t);
            }
        }
    } else {
        encode_block(
            iter,
            block,
            level / 2,
            x_offset,
            y_offset,
            &mut output,
            &mut output_test,
        );
    }
    y_offset += (level / 2);
    x_offset -= (level / 2);
    if *c {
        output.append(&mut BitVec::<u8, Msb0>::from_slice(
            &block[raw_block_idx(y_offset, x_offset)]
                .unwrap()
                .d
                .to_be_bytes(),
        ));
        for y in y_offset..y_offset + (level / 2) {
            for x in x_offset..x_offset + (level / 2) {
                output.append(&mut BitVec::<u8, Msb0>::from_slice(
                    &block[raw_block_idx(y, x)].unwrap().delta_t.to_be_bytes(),
                ));
                output_test.push(block[raw_block_idx(y, x)].unwrap().delta_t);
            }
        }
    } else {
        encode_block(
            iter,
            block,
            level / 2,
            x_offset,
            y_offset,
            &mut output,
            &mut output_test,
        );
    }
    x_offset += (level / 2);
    if *d {
        output.append(&mut BitVec::<u8, Msb0>::from_slice(
            &block[raw_block_idx(y_offset, x_offset)]
                .unwrap()
                .d
                .to_be_bytes(),
        ));
        for y in y_offset..y_offset + (level / 2) {
            for x in x_offset..x_offset + (level / 2) {
                output.append(&mut BitVec::<u8, Msb0>::from_slice(
                    &block[raw_block_idx(y, x)].unwrap().delta_t.to_be_bytes(),
                ));
                output_test.push(block[raw_block_idx(y, x)].unwrap().delta_t);
            }
        }
    } else {
        encode_block(
            iter,
            block,
            level / 2,
            x_offset,
            y_offset,
            &mut output,
            &mut output_test,
        );
    }
    // dbg!(output_test);
}

fn decode_block(
    mut iter_tree: &mut Iter<u8, Msb0>,
    mut events: &mut BitVec<u8, Msb0>,
    mut events_pos: &mut usize,
    block: &mut Block,
    level: usize,
    mut x_offset: usize,
    mut y_offset: usize,
) {
    let a = iter_tree.next().unwrap();
    let b = iter_tree.next().unwrap();
    let c = iter_tree.next().unwrap();
    let d = iter_tree.next().unwrap();

    if level == 2 {
        for y in y_offset..y_offset + (level) {
            for x in x_offset..x_offset + (level) {
                let mut d: D = events[*events_pos..*events_pos + 8].load_be();
                *events_pos += 8;
                let delta_t: DeltaT = events[*events_pos..*events_pos + 32].load_be();
                *events_pos += 32;
                block[raw_block_idx(y, x)] = Some(EventCoordless { d, delta_t });
            }
        }
        return;
    }

    if *a {
        let mut d: D = events[*events_pos..*events_pos + 8].load_be();
        *events_pos += 8;
        for y in y_offset..y_offset + (level / 2) {
            for x in x_offset..x_offset + (level / 2) {
                let delta_t: DeltaT = events[*events_pos..*events_pos + 32].load_be();
                *events_pos += 32;
                block[raw_block_idx(y, x)] = Some(EventCoordless { d, delta_t });
            }
        }
    } else {
        decode_block(
            iter_tree,
            events,
            events_pos,
            block,
            level / 2,
            x_offset,
            y_offset,
        );
    }
    x_offset += (level / 2);
    if *b {
        let mut d: D = events[*events_pos..*events_pos + 8].load_be();
        *events_pos += 8;
        for y in y_offset..y_offset + (level / 2) {
            for x in x_offset..x_offset + (level / 2) {
                let delta_t: DeltaT = events[*events_pos..*events_pos + 32].load_be();
                *events_pos += 32;
                block[raw_block_idx(y, x)] = Some(EventCoordless { d, delta_t });
            }
        }
    } else {
        decode_block(
            iter_tree,
            events,
            events_pos,
            block,
            level / 2,
            x_offset,
            y_offset,
        );
    }
    y_offset += (level / 2);
    x_offset -= (level / 2);
    if *c {
        let mut d: D = events[*events_pos..*events_pos + 8].load_be();
        *events_pos += 8;
        for y in y_offset..y_offset + (level / 2) {
            for x in x_offset..x_offset + (level / 2) {
                let delta_t: DeltaT = events[*events_pos..*events_pos + 32].load_be();
                *events_pos += 32;
                block[raw_block_idx(y, x)] = Some(EventCoordless { d, delta_t });
            }
        }
    } else {
        decode_block(
            iter_tree,
            events,
            events_pos,
            block,
            level / 2,
            x_offset,
            y_offset,
        );
    }
    x_offset += (level / 2);
    if *d {
        let mut d: D = events[*events_pos..*events_pos + 8].load_be();
        *events_pos += 8;
        for y in y_offset..y_offset + (level / 2) {
            for x in x_offset..x_offset + (level / 2) {
                let delta_t: DeltaT = events[*events_pos..*events_pos + 32].load_be();
                *events_pos += 32;
                block[raw_block_idx(y, x)] = Some(EventCoordless { d, delta_t });
            }
        }
    } else {
        decode_block(
            iter_tree,
            events,
            events_pos,
            block,
            level / 2,
            x_offset,
            y_offset,
        );
    }
}

///
/// ```
///
///
///
///
/// ```
fn by_2_2(block: &Block) -> BitVec<u8, Msb0> {
    let mut bv = bitvec![u8, Msb0;];

    // Traverse the block and build the tree
    // Top left pixel at each sub-block is the reference pixel
    for y in (0..BLOCK_SIZE).step_by(2) {
        for x in (0..BLOCK_SIZE).step_by(2) {
            let a = block[raw_block_idx(y, x)].unwrap();
            let b = block[raw_block_idx(y, x + 1)].unwrap();
            let c = block[raw_block_idx(y + 1, x)].unwrap();
            let d = block[raw_block_idx(y + 1, x + 1)].unwrap();
            bv.push(true);
            bv.push(a.d == b.d);
            bv.push(a.d == c.d);
            bv.push(a.d == d.d);
        }
    }

    bv
}

fn by_n_n(bv_2_2: BitVec<u8, Msb0>, divisor: usize) -> BitVec<u8, Msb0> {
    let mut bv_n_n = bitvec![u8, Msb0;];
    let mut iter = bv_2_2.iter();

    let mut bv_end = bitvec![u8, Msb0;];
    for i in 0..(BLOCK_SIZE / divisor) * (BLOCK_SIZE / divisor) {
        let a = iter.next().unwrap();
        let b = iter.next().unwrap();
        let c = iter.next().unwrap();
        let d = iter.next().unwrap();

        match *a && *b && *c && *d {
            true => {
                bv_n_n.push(true);
            }
            false => {
                bv_n_n.push(false);
                bv_end.push(*a);
                bv_end.push(*b);
                bv_end.push(*c);
                bv_end.push(*d);
            }
        }
    }

    bv_n_n.append(&mut bv_end);
    loop {
        let tmp = iter.next();
        match tmp {
            Some(val) => bv_n_n.push(*val),
            None => break,
        }
    }
    bv_n_n
}

#[inline(always)]
fn raw_block_idx(y: usize, x: usize) -> usize {
    y * BLOCK_SIZE + x
}

#[cfg(test)]
mod tests {
    use crate::codec::compressed::{
        by_2_2, decode_block, encode_block, raw_block_idx, Block, Cube, BLOCK_SIZE,
    };
    use crate::framer::driver::EventCoordless;
    use crate::{DeltaT, Event};
    use bitvec::prelude::*;
    use std::error::Error;
    use std::fs;
    use std::path::PathBuf;
    use std::process::Command;
    use std::thread::sleep;
    use std::time::Duration;

    fn setup_by_2_2() -> (BitVec<u8, Msb0>, Block) {
        let mut block = [None; BLOCK_SIZE * BLOCK_SIZE];
        let mut dummy_event = EventCoordless::default();

        for (idx, event) in block.iter_mut().enumerate() {
            *event = Some(EventCoordless {
                d: 0,
                delta_t: idx as DeltaT,
            });
        }

        dummy_event.d = 7;
        block[raw_block_idx(0, 0)].as_mut().unwrap().d = dummy_event.d;
        block[raw_block_idx(0, 1)].as_mut().unwrap().d = dummy_event.d;
        block[raw_block_idx(1, 0)].as_mut().unwrap().d = dummy_event.d;
        block[raw_block_idx(1, 1)].as_mut().unwrap().d = dummy_event.d;
        block[raw_block_idx(0, 2)].as_mut().unwrap().d = dummy_event.d;
        block[raw_block_idx(1, 2)].as_mut().unwrap().d = dummy_event.d;

        let bv = by_2_2(&block);
        (bv, block)
    }

    #[test]
    fn test_by_2_2() {
        let (bv, _) = setup_by_2_2();
        assert!(bv[0]);
        assert!(bv[1]);
        assert!(bv[2]);
        assert!(!bv[3]);
        assert!(bv[4]);
        assert!(!bv[5]);
        assert!(bv[6]);
    }

    #[test]
    fn test_by_4_4() {
        let (bv, _) = setup_by_2_2();

        let bv = super::by_n_n(bv, 2);

        assert_eq!(bv.len(), 1028); // 4 extra bits from the 2nd (2x2) block not being uniform

        assert!(bv[1024]);
        assert!(!bv[1025]);
        assert!(bv[1026]);
        assert!(!bv[1027]);
    }

    #[test]
    fn test_by_8_8() {
        let (bv, _) = setup_by_2_2();

        let bv = super::by_n_n(bv, 2);

        let bv = super::by_n_n(bv, 4);

        assert_eq!(bv.len(), 264); // 8 extra bits from the 2nd (2x2) block not being uniform

        assert!(!bv[0]);
        assert!(bv[256]);
        assert!(!bv[257]);
        assert!(bv[258]);
        assert!(bv[259]);
        assert!(bv[260]);
        assert!(!bv[261]);
        assert!(bv[262]);
        assert!(!bv[263]);
    }

    #[test]
    fn test_by_16_16() {
        let (bv, _) = setup_by_2_2();

        let bv = super::by_n_n(bv, 2);

        let bv = super::by_n_n(bv, 4);

        let bv = super::by_n_n(bv, 8);

        assert_eq!(bv.len(), 76); // 12 extra bits from the 2nd (2x2) block not being uniform

        assert!(!bv[0]);

        assert!(!bv[64]);
        assert!(bv[65]);
        assert!(bv[66]);
        assert!(bv[67]);

        assert!(bv[68]);
        assert!(!bv[69]);
        assert!(bv[70]);
        assert!(bv[71]);
        assert!(bv[72]);
        assert!(!bv[73]);
        assert!(bv[74]);
        assert!(!bv[75]);
    }

    #[test]
    fn test_by_32_32() {
        let (bv, _) = setup_by_2_2();

        let bv = super::by_n_n(bv, 2);

        let bv = super::by_n_n(bv, 4);

        let bv = super::by_n_n(bv, 8);

        let bv = super::by_n_n(bv, 16);

        assert_eq!(bv.len(), 32); // 16 extra bits from the 2nd (2x2) block not being uniform

        assert!(!bv[0]);

        assert!(!bv[16]);
        assert!(bv[17]);
        assert!(bv[18]);
        assert!(bv[19]);

        assert!(!bv[20]);
        assert!(bv[21]);
        assert!(bv[22]);
        assert!(bv[23]);

        assert!(bv[24]);
        assert!(!bv[25]);
        assert!(bv[26]);
        assert!(bv[27]);
        assert!(bv[28]);
        assert!(!bv[29]);
        assert!(bv[30]);
        assert!(!bv[31]);
    }

    #[test]
    fn test_by_64_64() {
        let (bv, _) = setup_by_2_2();

        let bv = super::by_n_n(bv, 2);

        let bv = super::by_n_n(bv, 4);

        let bv = super::by_n_n(bv, 8);

        let bv = super::by_n_n(bv, 16);

        let bv = super::by_n_n(bv, 32);

        assert_eq!(bv.len(), 24); // 20 extra bits from the 2nd (2x2) block not being uniform

        assert!(!bv[0]);

        assert!(!bv[4]);
        assert!(bv[5]);
        assert!(bv[6]);
        assert!(bv[7]);

        assert!(!bv[8]);
        assert!(bv[9]);
        assert!(bv[10]);
        assert!(bv[11]);

        assert!(!bv[12]);
        assert!(bv[13]);
        assert!(bv[14]);
        assert!(bv[15]);

        assert!(bv[16]);
        assert!(!bv[17]);
        assert!(bv[18]);
        assert!(bv[19]);
        assert!(bv[20]);
        assert!(!bv[21]);
        assert!(bv[22]);
        assert!(!bv[23]);
    }

    #[test]
    fn test_encode_decode_block() {
        let (bv, block) = setup_by_2_2();
        assert_eq!(block[2].unwrap().d, 7);

        let bv = super::by_n_n(bv, 2);

        let bv = super::by_n_n(bv, 4);

        let bv = super::by_n_n(bv, 8);

        let bv = super::by_n_n(bv, 16);

        let bv = super::by_n_n(bv, 32);

        let mut iter = bv.iter();

        let mut output = BitVec::new();
        let mut output_test = Vec::new();

        encode_block(
            &mut iter,
            &block,
            BLOCK_SIZE,
            0,
            0,
            &mut output,
            &mut output_test,
        );

        output_test.sort();
        output_test.dedup();
        assert_eq!(output_test.len(), BLOCK_SIZE * BLOCK_SIZE);

        let mut iter_tree = bv.iter();
        let mut events = output;
        let mut output_block = [None; BLOCK_SIZE * BLOCK_SIZE];
        let mut events_pos = 0;
        decode_block(
            &mut iter_tree,
            &mut events,
            &mut 0,
            &mut output_block,
            BLOCK_SIZE,
            0,
            0,
        );

        for y in 0..BLOCK_SIZE {
            for x in 0..BLOCK_SIZE {
                assert_eq!(
                    output_block[raw_block_idx(y, x)],
                    block[raw_block_idx(y, x)]
                );
            }
        }

        assert_eq!(output_block, block);
    }
}
