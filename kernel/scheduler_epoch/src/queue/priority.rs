use alloc::collections::VecDeque;
use core::{intrinsics::unlikely, slice};

use crate::{queue::RunQueue, EpochTaskRef};

pub(crate) struct Iter<'a> {
    inner: &'a RunQueue,
    mask: u64,
}

impl<'a> Iter<'a> {
    pub(crate) fn new(run_queue: &'a RunQueue) -> Self {
        Self {
            inner: run_queue,
            mask: u64::MAX,
        }
    }
}

impl<'a> Iterator for Iter<'a> {
    type Item = &'a VecDeque<EpochTaskRef>;

    fn next(&mut self) -> Option<Self::Item> {
        let next_index = (self.inner.priorities & self.mask).leading_zeros();

        if next_index == 64 {
            None
        } else {
            // https://users.rust-lang.org/t/how-to-make-an-integer-with-n-bits-set-without-overflow/63078
            self.mask = u64::MAX.checked_shr(next_index + 1).unwrap_or(0);
            Some(&self.inner.inner[next_index as usize])
        }
    }
}

pub(crate) struct IterMut<'a> {
    current_index: i8,
    priorities: u64,
    inner: slice::IterMut<'a, VecDeque<EpochTaskRef>>,
}

impl<'a> IterMut<'a> {
    pub(crate) fn new(run_queue: &'a mut RunQueue) -> Self {
        Self {
            current_index: -1,
            priorities: run_queue.priorities,
            inner: run_queue.inner.iter_mut(),
        }
    }
}

impl<'a> Iterator for IterMut<'a> {
    type Item = &'a mut VecDeque<EpochTaskRef>;

    fn next(&mut self) -> Option<Self::Item> {
        // https://users.rust-lang.org/t/how-to-make-an-integer-with-n-bits-set-without-overflow/63078
        if unlikely(self.current_index == 64) {
            None
        } else {
            let mask = u64::MAX
                .checked_shr((self.current_index + 1) as u32)
                .unwrap_or(0);
            let next_index = (self.priorities & mask).leading_zeros() as i8;

            let diff = next_index - self.current_index + 1;
            self.current_index = next_index;

            self.inner.nth(diff as usize)
        }
    }
}
