use crate::framer::driver::EventCoordless;
use crate::{DeltaT, Event, D};
use bitvec::prelude::*;
use bitvec::slice::Iter;
use ndarray::Array2;
use std::iter::Enumerate;

#[derive(Clone)]
struct SubBlock<'a> {
    pub(crate) a: Box<EventCoordlessBlock<'a>>,
    pub(crate) b: Box<EventCoordlessBlock<'a>>,
    pub(crate) c: Box<EventCoordlessBlock<'a>>,
    pub(crate) d: Box<EventCoordlessBlock<'a>>,
}

// impl IntoIterator for SubBlock {
//     type Item = Box<EventCoordlessBlock>;
//     type IntoIter = SubBlockIntoIterator;
//
//     fn into_iter(self) -> Self::IntoIter {
//         SubBlockIntoIterator {
//             sub_block: self,
//             index: 0,
//         }
//     }
// }
//
// pub struct SubBlockIntoIterator {
//     sub_block: SubBlock,
//     index: usize,
// }
//
// impl Iterator for SubBlockIntoIterator {
//     type Item = Box<EventCoordlessBlock>;
//     fn next(&mut self) -> Option<&mut Box<EventCoordlessBlock>> {
//         let result = match self.index {
//             0 => &mut self.sub_block.a,
//             1 => &mut self.sub_block.b,
//             2 => &mut self.sub_block.c,
//             3 => &mut self.sub_block.d,
//             _ => return None,
//         };
//         self.index += 1;
//         Some(result)
//     }
// }

#[derive(Clone)]
struct EventCoordlessBlock<'a> {
    sub_block: Option<SubBlock<'a>>,
    d_val: Option<D>,
    event_ref: Option<&'a Option<EventCoordless>>,
}

#[cfg(test)]
mod tests {
    use crate::codec::compressed::mod2::{EventCoordlessBlock, SubBlock};
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

    /// Create blocking tree of size `BLOCK_SIZE`
    fn setup_by_2_2(tree_block: &Block) -> SubBlock {
        // This is the base 2x2 sub block
        let mut base_sub_block = SubBlock {
            a: Box::new(EventCoordlessBlock {
                sub_block: None,
                d_val: Some(7),
                event_ref: Some(&tree_block[0]),
            }),
            b: Box::new(EventCoordlessBlock {
                sub_block: None,
                d_val: Some(7),
                event_ref: Some(&tree_block[0]),
            }),
            c: Box::new(EventCoordlessBlock {
                sub_block: None,
                d_val: Some(7),
                event_ref: Some(&tree_block[0]),
            }),
            d: Box::new(EventCoordlessBlock {
                sub_block: None,
                d_val: Some(7),
                event_ref: Some(&tree_block[0]),
            }),
        };

        // 4x4 layer
        let mut sub_block_4x4 = SubBlock {
            a: Box::new(EventCoordlessBlock {
                sub_block: Some(base_sub_block.clone()),
                d_val: None,
                event_ref: None,
            }),
            b: Box::new(EventCoordlessBlock {
                sub_block: Some(base_sub_block.clone()),
                d_val: None,
                event_ref: None,
            }),
            c: Box::new(EventCoordlessBlock {
                sub_block: Some(base_sub_block.clone()),
                d_val: None,
                event_ref: None,
            }),
            d: Box::new(EventCoordlessBlock {
                sub_block: Some(base_sub_block.clone()),
                d_val: None,
                event_ref: None,
            }),
        };

        // 8x8 layer
        let mut sub_block_8x8 = SubBlock {
            a: Box::new(EventCoordlessBlock {
                sub_block: Some(sub_block_4x4.clone()),
                d_val: None,
                event_ref: None,
            }),
            b: Box::new(EventCoordlessBlock {
                sub_block: Some(sub_block_4x4.clone()),
                d_val: None,
                event_ref: None,
            }),
            c: Box::new(EventCoordlessBlock {
                sub_block: Some(sub_block_4x4.clone()),
                d_val: None,
                event_ref: None,
            }),
            d: Box::new(EventCoordlessBlock {
                sub_block: Some(sub_block_4x4.clone()),
                d_val: None,
                event_ref: None,
            }),
        };
        // 16x16 layer
        let mut sub_block_16x16 = SubBlock {
            a: Box::new(EventCoordlessBlock {
                sub_block: Some(sub_block_8x8.clone()),
                d_val: None,
                event_ref: None,
            }),
            b: Box::new(EventCoordlessBlock {
                sub_block: Some(sub_block_8x8.clone()),
                d_val: None,
                event_ref: None,
            }),
            c: Box::new(EventCoordlessBlock {
                sub_block: Some(sub_block_8x8.clone()),
                d_val: None,
                event_ref: None,
            }),
            d: Box::new(EventCoordlessBlock {
                sub_block: Some(sub_block_8x8.clone()),
                d_val: None,
                event_ref: None,
            }),
        };

        // 32x32 layer
        let mut sub_block_32x32 = SubBlock {
            a: Box::new(EventCoordlessBlock {
                sub_block: Some(sub_block_16x16.clone()),
                d_val: None,
                event_ref: None,
            }),
            b: Box::new(EventCoordlessBlock {
                sub_block: Some(sub_block_16x16.clone()),
                d_val: None,
                event_ref: None,
            }),
            c: Box::new(EventCoordlessBlock {
                sub_block: Some(sub_block_16x16.clone()),
                d_val: None,
                event_ref: None,
            }),
            d: Box::new(EventCoordlessBlock {
                sub_block: Some(sub_block_16x16.clone()),
                d_val: None,
                event_ref: None,
            }),
        };

        // 64x64 layer
        let mut sub_block_64x64 = SubBlock {
            a: Box::new(EventCoordlessBlock {
                sub_block: Some(sub_block_32x32.clone()),
                d_val: None,
                event_ref: None,
            }),
            b: Box::new(EventCoordlessBlock {
                sub_block: Some(sub_block_32x32.clone()),
                d_val: None,
                event_ref: None,
            }),
            c: Box::new(EventCoordlessBlock {
                sub_block: Some(sub_block_32x32.clone()),
                d_val: None,
                event_ref: None,
            }),
            d: Box::new(EventCoordlessBlock {
                sub_block: Some(sub_block_32x32.clone()),
                d_val: None,
                event_ref: None,
            }),
        };

        sub_block_64x64
    }

    #[test]
    fn test_by_2_2() {
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

        dummy_event.d = 4;
        block[raw_block_idx(2, 3)].as_mut().unwrap().d = dummy_event.d;
        block[raw_block_idx(3, 3)].as_mut().unwrap().d = dummy_event.d;

        let tree_block = setup_by_2_2(&block);

        // Test that the block is the correct size
        let mut layers = 0;
        let mut current_block = &tree_block;
        loop {
            current_block = match current_block.a.sub_block.as_ref() {
                None => break,
                Some(a) => a,
            };
            layers += 1;
        }
        assert_eq!(layers, 5);
    }
}
