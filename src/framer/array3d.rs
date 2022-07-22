use std::ops::{Index, IndexMut};
use std::slice::Iter;
use std::vec::IntoIter;
use itertools::{IntoChunks, Itertools};
use crate::framer::array3d::Array3DError::InvalidIndex;

pub struct Array3D<T> {
    array: Vec<T>,
    num_rows: usize,
    num_cols: usize,
    num_channels: usize
}


pub enum Array3DError {
    /// Index out of bounds
    InvalidIndex,
}

impl<T: Default + std::clone::Clone> Array3D<T> {

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
    pub fn set_at(&mut self, item: T, row: usize, col: usize, channel: usize) -> Result<&T, Array3DError>{
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

    pub fn iter_2d(&self) -> IntoChunks<Iter<'_, T>> {
        self.array.iter().chunks(self.num_channels)
    }
}

impl<T: Default + std::clone::Clone> Index<(usize, usize)> for Array3D<T> {
    type Output = T;

    fn index(&self, (row, col): (usize, usize)) -> &Self::Output {
        self.at(row, col, 0).unwrap_or_else(|| panic!("Invalid index for row {}, col {}", row, col))
    }
}

impl<T: Default + std::clone::Clone> IndexMut<(usize, usize)> for Array3D<T> {
    fn index_mut(&mut self, (row, col): (usize, usize)) -> &mut Self::Output {
        self.at_mut(row, col, 0).unwrap_or_else(|| panic!("Invalid index for row {}, col {}", row, col))
    }
}

impl<T: Default + std::clone::Clone> Index<(usize, usize, usize)> for Array3D<T> {
    type Output = T;

    fn index(&self, (row, col, channel): (usize, usize, usize)) -> &Self::Output {
        self.at(row, col, channel).unwrap_or_else(|| panic!("Invalid index for row {}, col {}, channel {}", row, col, channel))
    }
}

impl<T: Default + std::clone::Clone> IndexMut<(usize, usize, usize)> for Array3D<T> {
    fn index_mut(&mut self, (row, col, channel): (usize, usize, usize)) -> &mut Self::Output {
        self.at_mut(row, col, channel).unwrap_or_else(|| panic!("Invalid index for row {}, col {}, channel {}", row, col, channel))
    }
}