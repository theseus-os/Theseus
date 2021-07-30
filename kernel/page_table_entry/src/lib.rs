//! Defines the structure of Page Table Entries (PTEs) on x86_64.
//!
//! This crate is also useful for frame deallocation, because it can determine
//! when a frame is mapped exclusively to only one page table entry, 
//! and therefore when it is safe to deallocate. 
//! 
//! Because Theseus ensures a bijective (1-to-1) mapping
//! between virtual pages and physical frames,
//! it is almost always the case that the frame pointed to 
//! by a newly-unmapped page table entry can be deallocated. 
//!

#![no_std]

extern crate memory_structs;
extern crate bit_field;
extern crate kernel_config;
extern crate zerocopy;

use core::ops::Deref;
use memory_structs::{Frame, FrameRange, EntryFlags, PhysicalAddress};
use bit_field::BitField;
use kernel_config::memory::PAGE_SHIFT;
use zerocopy::FromBytes;

/// A page table entry, which is a `u64` value under the hood.
///
/// It contains a the physical address of the `Frame` being mapped by this entry
/// and the access bits (encoded `EntryFlags`) that describes how it's mapped,
/// e.g., readable, writable, no exec, etc. 
///
/// There isn't and shouldn't be any way to create/instantiate a new `PageTableEntry` directly.
/// You can only obtain a reference to an `PageTableEntry` by going through a page table's `Table` struct itself.
#[derive(FromBytes)]
#[repr(transparent)]
pub struct PageTableEntry(u64);

impl PageTableEntry {
    /// Returns `true` if this entry is unused, i.e., cleared/zeroed out.
    pub fn is_unused(&self) -> bool {
        self.0 == 0
    }

    /// Zeroes out this entry, setting it as "unused".
    pub fn zero(&mut self) {
        self.0 = 0;
    }

    /// Removes the mapping represented by this page table entry. 
    /// 
    /// If the frame(s) pointed to by this entry were mapped exlusively,
    /// i.e., owned by this entry and not mapped anywhere else by any other entries,
    /// then this function returns those frames. 
    /// This is useful because those returned frames can then be safely deallocated.
    pub fn set_unmapped(&mut self) -> UnmapResult {
        let frame = self.frame_value();
        let flags = self.flags();
        self.zero();

        // Since we don't support huge pages, this PTE can only cover one 4KiB frame. 
        // Once we support huge pages, we can use a type parameter 
        // to specify whether this is a 4KiB, 2MiB, or 1GiB PTE. 
        let frame_range = FrameRange::new(frame, frame);
        if flags.is_exclusive() {
            UnmapResult::Exclusive(UnmappedFrames(frame_range))
        } else {
            UnmapResult::NonExclusive(frame_range)
        }
    }

    /// Returns this `PageTableEntry`'s flags.
    pub fn flags(&self) -> EntryFlags {
        EntryFlags::from_bits_truncate(self.0)
    }

    /// Returns the physical `Frame` pointed to (mapped by) this `PageTableEntry`.
    /// If this page table entry is not `PRESENT`, this returns `None`. 
    pub fn pointed_frame(&self) -> Option<Frame> {
        if self.flags().intersects(EntryFlags::PRESENT) {
            Some(self.frame_value())
        } else {
            None
        }
    }

    /// Extracts the value of the frame referred to by this page table entry.
    fn frame_value(&self) -> Frame {
        let mut frame_paddr = self.0 as usize;
        frame_paddr.set_bits(0 .. (PAGE_SHIFT as u8), 0);
        Frame::containing_address(PhysicalAddress::new_canonical(frame_paddr))
    }

    /// Sets this `PageTableEntry` to map the given `Frame` with the given `flags`.
    ///
    /// This is the actual mapping action that informs the MMU of a new mapping.
    ///
    /// Note: this performs no checks about the current value of this page table entry. 
    pub fn set_entry(&mut self, frame: Frame, flags: EntryFlags) {
        self.0 = (frame.start_address().value() as u64) | flags.bits();
    }

    pub fn value(&self) -> u64 {
        self.0
    }
}

/// The frames returned from the action of unmapping a page table entry.
/// See the `PageTableEntry::set_unmapped()` function.
///
/// If exclusive, the contained `UnmappedFrames` can be used to deallocate frames. 
///
/// If non-exclusive, the contained `FrameRange` is provided just for debugging feedback.
/// Note that we use `FrameRange` instead of `Frame` because a single page table entry
/// can map many frames, e.g., using huge pages. 
#[must_use]
pub enum UnmapResult {
    Exclusive(UnmappedFrames),
    NonExclusive(FrameRange)
}

/// A range of frames that have been unmapped from a `PageTableEntry`
/// that previously mapped that frame exclusively (i.e., "owned it").
///
/// These `UnmappedFrames` can be converted into `AllocatedFrames`
/// and then safely deallocated.
///
/// See the `PageTableEntry::set_unmapped()` function.
pub struct UnmappedFrames(FrameRange);

impl Deref for UnmappedFrames {
    type Target = FrameRange;
    fn deref(&self) -> &FrameRange {
        &self.0
    }
}
