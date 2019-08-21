// Copyright 2016 Philipp Oppermann. See the README.md
// file at the top-level directory of this distribution.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

pub use super::super::EntryFlags;
use super::super::{Frame};
use PhysicalAddress;
use bit_field::BitField;
use kernel_config::memory::PAGE_SHIFT;


/// A page table entry, which is a `u64` value under the hood.
/// It contains a physical frame address and entry flag access bits.
#[repr(transparent)]
pub struct Entry(u64);

impl Entry {
    pub fn is_unused(&self) -> bool {
        self.0 == 0
    }

    pub fn set_unused(&mut self) {
        self.0 = 0;
    }

    pub fn flags(&self) -> EntryFlags {
        EntryFlags::from_bits_truncate(self.0)
    }

    pub fn pointed_frame(&self) -> Option<Frame> {
        if self.flags().contains(EntryFlags::PRESENT) {
            let mut frame_paddr = self.0 as usize;
            frame_paddr.set_bits(0 .. (PAGE_SHIFT as u8), 0);
            Some(Frame::containing_address(PhysicalAddress::new_canonical(frame_paddr)))
        } else {
            None
        }
    }

    pub fn set(&mut self, frame: Frame, flags: EntryFlags) {
        self.0 = (frame.start_address().value() as u64) | flags.bits();
    }

    // we use this to force explicit copying rather than deriving Copy/Clone
    pub fn copy(&self) -> Entry {
        Entry(self.0)
    }

    pub fn value(&self) -> u64 {
        self.0
    }
}
