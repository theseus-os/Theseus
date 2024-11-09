use core::intrinsics::unlikely;

/// An iterator over a [`BitSet`].
///
/// [`BitSet`]: crate::BitSet
pub struct Iter {
    set: u64,
    current_mask: u64,
}

impl Iter {
    pub(crate) const fn new(set: u64) -> Self {
        Self {
            set,
            current_mask: u64::MAX,
        }
    }
}

impl Iterator for Iter {
    type Item = usize;

    fn next(&mut self) -> Option<Self::Item> {
        let next_index = (self.set & self.current_mask).trailing_zeros();

        if unlikely(next_index == 64) {
            None
        } else {
            // https://users.rust-lang.org/t/how-to-make-an-integer-with-n-bits-set-without-overflow/63078
            self.current_mask = u64::MAX.checked_shl(next_index + 1).unwrap_or(0);
            Some(next_index as usize)
        }
    }
}

#[cfg(test)]
mod tests {
    extern crate alloc;

    use alloc::vec::Vec;

    use crate::BitSet;

    #[test]
    fn test_iter() {
        let mut set = BitSet::new();
        set.insert(57);
        set.insert(58);
        set.insert(61);
        set.insert(63);
        assert_eq!(set.iter().collect::<Vec<_>>(), [57, 58, 61, 63]);

        let mut set = BitSet::new();
        set.insert(0);
        set.insert(8);
        set.insert(16);
        set.insert(24);
        set.insert(32);
        set.insert(40);
        set.insert(48);
        set.insert(56);
        assert_eq!(
            set.iter().collect::<Vec<_>>(),
            [0, 8, 16, 24, 32, 40, 48, 56]
        );
    }
}
