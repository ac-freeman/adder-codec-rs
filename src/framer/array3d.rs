use std::io::{BufWriter, Write};
use std::mem::size_of;
use std::ops::{Index, IndexMut};
use std::slice::{Iter, IterMut};
use std::vec::IntoIter;
use bytes::{BufMut, Bytes, BytesMut};
use itertools::{IntoChunks, Itertools};
use crate::{Event, EventCoordless};
use crate::framer::array3d::Array3DError::InvalidIndex;

#[derive(Debug, Clone, Default)]
pub struct Array3D<T> {
    array: Vec<T>,
    num_rows: usize,
    num_cols: usize,
    num_channels: usize
}

#[derive(Debug)]
pub enum Array3DError {
    /// Index out of bounds
    InvalidIndex,
}

impl<T: Default + std::clone::Clone> Array3D<T> where bytes::Bytes: From<Vec<u8>> {

    /// Allocates a new [`Array3D`], initializing all elements with defaults
    ///
    /// # Examples
    ///
    /// ```
    /// # use adder_codec_rs::Event;
    /// # use adder_codec_rs::framer::array3d::{Array3D};
    /// let arr: Array3D<Event> = Array3D::new(10, 10, 3);
    /// ```
    pub fn new(num_rows: usize, num_cols: usize, num_channels: usize) -> Self {
        let arr: Vec<T> = vec![T::default(); num_rows * num_cols * num_channels];

        Array3D {
            array: arr,
            num_rows,
            num_cols,
            num_channels
        }
    }

    pub fn new_like(ref_array: &Array3D<T>) -> Self {
        let array: Array3D<T> = Array3D::new(ref_array.num_rows, ref_array.num_cols, ref_array.num_channels);
        array
    }

    pub fn num_rows(&self) -> usize {
        self.num_rows
    }

    pub fn num_cols(&self) -> usize {
        self.num_cols
    }

    pub fn num_channels(&self) -> usize {
        self.num_channels
    }

    pub fn num_elems(&self) -> usize {
        self.num_rows * self.num_cols * self.num_channels
    }

    fn check_idx(&self, row: usize, col: usize, channel: usize) -> Result<(), Array3DError> {
        if row < self.num_rows && col < self.num_cols && channel < self.num_channels {
            return Ok(())
        }
        Err(InvalidIndex)
    }

    pub fn at(&self, row: usize, col: usize, channel: usize) -> Option<&T> {
        match self.check_idx(row, col, channel) {
            Ok(_) => {
                Some(&self.array[row * (self.num_cols * self.num_channels) + col * self.num_channels + channel])
            }
            Err(_) => {
                None
            }
        }
    }


    /// Gets a mutable reference to an element
    ///
    /// # Examples
    ///
    /// ```
    /// # use adder_codec_rs::Event;
    /// # use adder_codec_rs::framer::array3d::{Array3D};
    /// let mut  arr: Array3D<u8> = Array3D::new(10, 10, 3);
    /// let elem = arr.at_mut(0,0,0).unwrap();
    /// *elem = 255;
    /// ```
    pub fn at_mut(&mut self, row: usize, col: usize, channel: usize) -> Option<&mut T> {
        match self.check_idx(row, col, channel) {
            Ok(_) => {
                Some(&mut self.array[row * (self.num_cols * self.num_channels) + col * self.num_channels + channel])
            }
            Err(_) => {
                None
            }
        }
    }

    /// Set the element of the array at index \[row, col, channel\] to be this [item]. If
    /// successful, returns a mutable reference to the moved element.
    ///
    /// # Examples
    ///
    /// ```
    /// # use adder_codec_rs::Event;
    /// # use adder_codec_rs::framer::array3d::{Array3D, Array3DError};
    /// let mut  arr: Array3D<u8> = Array3D::new(10, 10, 3);
    /// let elem: &mut u8 = match arr.set_at(255, 0,0,0) {Ok(a) => {a}Err(_) => {panic!()}};
    /// *elem = 10;
    /// ```
    pub fn set_at(&mut self, item: T, row: usize, col: usize, channel: usize) -> Result<&mut T, Array3DError>{
        match self.check_idx(row, col, channel) {
            Ok(_) => {
                let elem = &mut self.array[row * (self.num_cols * self.num_channels) + col * self.num_channels + channel];
                *elem = item;
                Ok(elem)
            }
            Err(e) => {
                return Err(e)
            }
        }
    }

    /// Immutably iterate the [`Array3D`] across the first two dimensions. For example, if there are
    /// 3 [channels], the first element of the returned iterator will be an iterator over the
    /// 3 values stored at index \[0, 0\].
    ///
    /// # Examples
    ///
    /// ```
    /// # use adder_codec_rs::Event;
    /// # use adder_codec_rs::framer::array3d::{Array3D, Array3DError};
    /// let mut  arr: Array3D<u16> = Array3D::new(10, 10, 3);
    /// arr.set_at(100, 0,0,0);
    /// arr.set_at(250, 0,0,1);
    /// arr.set_at(325,0,0,2);
    /// for elem in &arr.iter_2d() {
    ///     let first_sum = elem.sum::<u16>();  // Just summing the first element to show an example
    ///     assert_eq!(first_sum, 675);
    ///     break;
    /// }
    /// ```
    pub fn iter_2d(&self) -> IntoChunks<Iter<'_, T>> {
        self.array.iter().chunks(self.num_channels)
    }

    /// Mutably iterate the [`Array3D`] across the first two dimensions. For example, if there are
    /// 3 [channels], the first element of the returned iterator will be an iterator over the
    /// 3 values stored at index \[0, 0\].
    ///
    /// # Examples
    ///
    /// ```
    /// # use adder_codec_rs::Event;
    /// # use adder_codec_rs::framer::array3d::{Array3D, Array3DError};
    /// let mut  arr: Array3D<u16> = Array3D::new(10, 10, 3);
    /// arr.set_at(100, 0,0,0);
    /// arr.set_at(250, 0,0,1);
    /// arr.set_at(325,0,0,2);
    /// for mut elem in &arr.iter_2d_mut() {
    ///     for i in elem {
    ///         *i = *i + 1;
    ///     }
    ///     break;
    /// }
    /// for elem in &arr.iter_2d() {
    ///     let first_sum = elem.sum::<u16>();
    ///     assert_eq!(first_sum, 678);
    ///     break;
    /// }
    pub fn iter_2d_mut(&mut self) -> IntoChunks<IterMut<'_, T>> {
        self.array.iter_mut().chunks(self.num_channels)
    }

    // pub fn serialize_to_be_bytes(&self) ->  BytesMut {
    //     let mut buf = BytesMut::with_capacity(self.num_rows*self.num_cols*self.num_channels * size_of::<T>());
    //     for elem in &self.iter_2d() {
    //         for sub_elem in elem {
    //             // sub_elem.to_be_bytes();
    //             buf.put(Bytes::from(sub_elem.clone()));
    //         }
    //     }
    //
    //     buf
    // }
}

impl Array3D<Option<EventCoordless>> {
    pub fn serialize_to_be_bytes(&self) ->  BytesMut {
        let mut buf = BytesMut::with_capacity(self.num_rows*self.num_cols*self.num_channels * size_of::<EventCoordless>());
        for elem in &self.iter_2d() {
            for sub_elem in elem {
                // sub_elem.to_be_bytes();
                match sub_elem {
                    None => {
                        let empty = EventCoordless { d: 0, delta_t: 0 };
                        buf.put(Bytes::from(&empty))
                    }
                    Some(event) => {
                        buf.put(Bytes::from(event))
                    }
                }
            }
        }

        buf
    }
}

use duplicate::duplicate_item;
#[duplicate_item(name; [u8]; [u16]; [u32]; [u64];)]
impl Array3D<name> {
    pub fn serialize_to_be_bytes(&self) ->  BytesMut {
        let mut buf = BytesMut::with_capacity(self.num_rows*self.num_cols*self.num_channels * size_of::<name>());
        for elem in &self.iter_2d() {
            for sub_elem in elem {
                buf.put(Bytes::from(sub_elem.to_be_bytes().to_vec()));
            }
        }
        buf
    }
}



impl<T: Default + std::clone::Clone > Index<(usize, usize)> for Array3D<T> where bytes::Bytes: From<T> {
    type Output = T;

    fn index(&self, (row, col): (usize, usize)) -> &Self::Output {
        self.at(row, col, 0).unwrap_or_else(|| panic!("Invalid index for row {}, col {}", row, col))
    }
}

impl<T: Default + std::clone::Clone> IndexMut<(usize, usize)> for Array3D<T> where bytes::Bytes: From<T> {
    fn index_mut(&mut self, (row, col): (usize, usize)) -> &mut Self::Output {
        self.at_mut(row, col, 0).unwrap_or_else(|| panic!("Invalid index for row {}, col {}", row, col))
    }
}

impl<T: Default + std::clone::Clone> Index<(usize, usize, usize)> for Array3D<T> where bytes::Bytes: From<T> {
    type Output = T;

    fn index(&self, (row, col, channel): (usize, usize, usize)) -> &Self::Output {
        self.at(row, col, channel).unwrap_or_else(|| panic!("Invalid index for row {}, col {}, channel {}", row, col, channel))
    }
}

impl<T: Default + std::clone::Clone> IndexMut<(usize, usize, usize)> for Array3D<T> where bytes::Bytes: From<T> {
    fn index_mut(&mut self, (row, col, channel): (usize, usize, usize)) -> &mut Self::Output {
        self.at_mut(row, col, channel).unwrap_or_else(|| panic!("Invalid index for row {}, col {}, channel {}", row, col, channel))
    }
}

// impl<T: Default + Clone + From<T>> From<&Array3D<T>> for Bytes{
//     fn from(arr: &Array3D<T>) -> Self {
//         let mut buff = Vec::with_capacity(arr.num_rows*arr.num_cols*arr.num_channels * size_of::<T>());
//         for elem in &arr.iter_2d() {
//             for sub_elem in elem {
//                 buff.push(&Bytes::from(sub_elem.clone()).to_vec());
//             }
//         }
//
//         let t = Bytes::from(buff.concat());
//         t
//     }
// }