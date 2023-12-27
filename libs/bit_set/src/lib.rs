#![no_std]
#![feature(const_likely, core_intrinsics)]

mod iter;

use core::intrinsics::likely;

pub use iter::Iter;

/// A bit set backed by a [`u64`].
///
/// This is equivalent to a `HashSet<u8>` storing integers in the range `[0,
/// 64)`.
#[derive(Debug, Clone)]
pub struct BitSet {
    inner: u64,
}

impl BitSet {
    /// Constructs a new, empty `BitSet`.
    pub const fn new() -> Self {
        Self { inner: 0 }
    }

    /// Returns an iterator over the elements of the set.
    #[must_use]
    pub const fn iter(&self) -> Iter {
        Iter::new(self.inner)
    }

    /// Returns `true` if the set contains the given element.
    ///
    /// # Panics
    ///
    /// Panics if `element` is greater than 63.
    #[must_use]
    pub const fn contains(&self, element: u8) -> bool {
        assert!(element < 64);
        self.inner & (1 << element) != 0
    }

    /// Adds an element to the set.
    ///
    /// # Panics
    ///
    /// Panics if `element` is greater than 63.
    pub fn insert(&mut self, element: u8) {
        assert!(element < 64);
        self.inner |= 1 << element;
    }

    /// Removes an element from the set.
    ///
    /// # Panics
    ///
    /// Panics if `element` is greater than 63.
    pub fn remove(&mut self, element: u8) {
        assert!(element < 64);
        self.inner &= !(1 << element);
    }

    /// Returns the smallest element in the set.
    ///
    /// Returns `None` if the set is empty.
    #[must_use]
    pub const fn min(&self) -> Option<u8> {
        if likely(self.inner != 0) {
            Some(self.inner.trailing_zeros() as u8)
        } else {
            None
        }
    }

    /// Returns the largest element in the set.
    ///
    /// Returns `None` if the set is empty.
    #[must_use]
    pub const fn max(&self) -> Option<u8> {
        if likely(self.inner != 0) {
            // self.inner.leading_zeros() <= 63
            Some(63 - self.inner.leading_zeros() as u8)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_contains() {
        let mut set = BitSet::new();

        for i in 0..64 {
            assert!(!set.contains(i));
        }

        set.insert(3);

        for i in 0..64 {
            if i != 3 {
                assert!(!set.contains(i));
            } else {
                assert!(set.contains(i));
            }
        }

        set.insert(0);

        for i in 0..64 {
            if i != 0 && i != 3 {
                assert!(!set.contains(i));
            } else {
                assert!(set.contains(i));
            }
        }

        set.insert(63);

        for i in 0..64 {
            if i != 0 && i != 3 && i != 63 {
                assert!(!set.contains(i));
            } else {
                assert!(set.contains(i));
            }
        }
    }

    #[test]
    fn test_remove() {
        let mut set = BitSet::new();

        set.insert(3);
        set.insert(63);
        set.remove(3);

        for i in 0..64 {
            if i != 63 {
                assert!(!set.contains(i));
            } else {
                assert!(set.contains(i));
            }
        }
    }

    #[test]
    fn test_min_max() {
        let mut set = BitSet::new();
        assert_eq!(set.min(), None);
        assert_eq!(set.max(), None);

        set.insert(5);
        assert_eq!(set.min(), Some(5));
        assert_eq!(set.max(), Some(5));

        set.insert(3);
        assert_eq!(set.min(), Some(3));
        assert_eq!(set.max(), Some(5));
    }
}
