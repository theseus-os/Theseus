//! Provides an array that distributes its elements as evenly as possible across
//! multiple cache lines without increasing the memory footprint.
//!
//! The cache line size is assumed to be 64 bytes.
//!
//! For more information see [`CacheLineDistributedArray`].

#![feature(const_trait_impl)]
#![no_std]

use core::{mem, ops};

const LINE_SIZE: usize = 64;

/// An array that distributes its elements as evenly as possible across multiple
/// cache lines without increasing the memory footprint.
///
/// It spreads elements across as many cache lines as possible. For example if
/// only the first four elements of a 256 byte array are being accessed, each
/// element will have its own cache line.
///
/// Indexing is faster if the number of cache lines that the array spans is a
/// power of two.
///
/// The cache line size is assumed to be 64 bytes.
#[derive(Debug)]
// Alignment must be the same as `LINE_SIZE`.
#[repr(align(64))]
pub struct CacheLineDistributedArray<T, const N: usize> {
    inner: [T; N],
}

impl<T, const N: usize> const From<[T; N]> for CacheLineDistributedArray<T, N> {
    fn from(value: [T; N]) -> Self {
        Self { inner: value }
    }
}

impl<T, const N: usize> ops::Index<usize> for CacheLineDistributedArray<T, N> {
    type Output = T;

    fn index(&self, index: usize) -> &Self::Output {
        &self.inner[Self::map_index(index)]
    }
}

impl<T, const N: usize> ops::IndexMut<usize> for CacheLineDistributedArray<T, N> {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        &mut self.inner[Self::map_index(index)]
    }
}

impl<T, const N: usize> CacheLineDistributedArray<T, N>
where
    T: Copy,
{
    pub fn new(value: T) -> Self {
        Self { inner: [value; N] }
    }
}

impl<T, const N: usize> CacheLineDistributedArray<T, N> {
    /// The size of the inner array must be a multiple of the line size or it
    /// must be one less than a multiple of the line size or it must only
    /// take up at most one cache line.
    const IS_CORRECT_SIZE: () = assert!(is_correct_size::<T, N>());

    /// An exact number of elements must fit in a cache line.
    const ELEMENT_ALIGNED: () = assert!(LINE_SIZE % mem::size_of::<T>() == 0);

    const NUM_LINES: usize = calculate_num_lines::<Self>();
    const NUM_ELEMENTS_IN_LINE: usize = LINE_SIZE / mem::size_of::<T>();

    fn map_index(index: usize) -> usize {
        let _ = Self::IS_CORRECT_SIZE;
        let _ = Self::ELEMENT_ALIGNED;

        assert!(index < N);
        let line_num = index % Self::NUM_LINES;
        let line_offset = index / Self::NUM_LINES;

        line_num * Self::NUM_ELEMENTS_IN_LINE + line_offset
    }
}

const fn calculate_num_lines<T>() -> usize {
    match mem::size_of::<T>() {
        0 => 0,
        size => ((size - 1) / LINE_SIZE) + 1,
    }
}

const fn is_correct_size<T, const N: usize>() -> bool {
    calculate_num_lines::<[T; N]>() == 1
        || mem::size_of::<[T; N]>() % LINE_SIZE == 0
        || mem::size_of::<[T; N]>() % LINE_SIZE == 63
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_distribution() {
        assert_eq!(CacheLineDistributedArray::<u64, 16>::map_index(0), 0);
        assert_eq!(CacheLineDistributedArray::<u64, 16>::map_index(1), 8);
        assert_eq!(CacheLineDistributedArray::<u64, 16>::map_index(2), 1);
        assert_eq!(CacheLineDistributedArray::<u64, 16>::map_index(3), 9);
        assert_eq!(CacheLineDistributedArray::<u64, 16>::map_index(4), 2);
        assert_eq!(CacheLineDistributedArray::<u64, 16>::map_index(5), 10);
        assert_eq!(CacheLineDistributedArray::<u64, 16>::map_index(6), 3);
        assert_eq!(CacheLineDistributedArray::<u64, 16>::map_index(7), 11);
        assert_eq!(CacheLineDistributedArray::<u64, 16>::map_index(8), 4);
        assert_eq!(CacheLineDistributedArray::<u64, 16>::map_index(9), 12);
        assert_eq!(CacheLineDistributedArray::<u64, 16>::map_index(10), 5);
        assert_eq!(CacheLineDistributedArray::<u64, 16>::map_index(11), 13);
        assert_eq!(CacheLineDistributedArray::<u64, 16>::map_index(12), 6);
        assert_eq!(CacheLineDistributedArray::<u64, 16>::map_index(13), 14);
        assert_eq!(CacheLineDistributedArray::<u64, 16>::map_index(14), 7);
        assert_eq!(CacheLineDistributedArray::<u64, 16>::map_index(15), 15);
    }

    #[test]
    fn test_indexing() {
        let mut array = CacheLineDistributedArray::from([0u8; 256]);
        for i in 0..256 {
            array[i] = 1;
        }
        assert!(array.inner.iter().all(|num| *num == 1));

        let mut array = CacheLineDistributedArray::from([0u16; 256]);
        for i in 0..256 {
            array[i] = 1;
        }
        assert!(array.inner.iter().all(|num| *num == 1));

        let mut array = CacheLineDistributedArray::from([0u32; 256]);
        for i in 0..256 {
            array[i] = 1;
        }
        assert!(array.inner.iter().all(|num| *num == 1));

        let mut array = CacheLineDistributedArray::from([0u64; 256]);
        for i in 0..256 {
            array[i] = 1;
        }
        assert!(array.inner.iter().all(|num| *num == 1));

        let mut array = CacheLineDistributedArray::from([0u128; 256]);
        for i in 0..256 {
            array[i] = 1;
        }
        assert!(array.inner.iter().all(|num| *num == 1));
    }

    #[test]
    #[should_panic]
    fn test_out_of_bounds() {
        CacheLineDistributedArray::<u64, 128>::map_index(128);
    }
}
