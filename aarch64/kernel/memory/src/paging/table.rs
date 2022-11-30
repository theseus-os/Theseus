// Copyright 2016 Philipp Oppermann. See the README.md
// file at the top-level directory of this distribution.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use super::PageTableEntry;
use kernel_config::memory::{PAGE_SHIFT, ENTRIES_PER_PAGE_TABLE};
use memory_structs::PAGE_TABLE_ENTRY_FRAME_MASK;
use super::super::{VirtualAddress, PteFlags};
use core::ops::{Index, IndexMut};
use core::marker::PhantomData;
use zerocopy::FromBytes;


/// Theseus uses the 511th entry of the P4 table for mapping the higher-half kernel, 
/// so it uses the 510th entry of P4 for the recursive mapping.
/// 
/// NOTE: this must be kept in sync with the recursive index in `kernel_config/memory.rs`
///       and `nano_core/<arch>/boot.asm`.
///
/// See these links for more: 
/// * <http://forum.osdev.org/viewtopic.php?f=1&p=176913>
/// * <http://forum.osdev.org/viewtopic.php?f=15&t=25545>
pub const P4: *mut Table<Level4> = 0o177777_776_776_776_776_0000 as *mut _; 
                                         // ^p4 ^p3 ^p2 ^p1 ^offset  
                                         // ^ 0o776 means that we're always looking at the 510th entry recursively

#[derive(FromBytes)]
pub struct Table<L: TableLevel> {
    entries: [PageTableEntry; ENTRIES_PER_PAGE_TABLE],
    level: PhantomData<L>,
}

impl<L: TableLevel> Table<L> {
    /// Zero out (clear) all entries in this page table frame. 
    pub(crate) fn zero(&mut self) {
        for entry in self.entries.iter_mut() {
            entry.zero();
        }
    }
}

impl<L: HierarchicalLevel> Table<L> {
    /// Uses the given `index` as an index into this table's list of entries.
    ///
    /// Returns the virtual address of the next lowest page table:
    /// if `self` is a P4-level `Table`, then this returns a P3-level `Table`,
    /// and so on for P3 -> P3 and P2 -> P1.
    /// 
    /// With `active` set to true, you indicate that the
    /// modified table is currently used by the CPU, in which case
    /// it will be accessed using the recursive paging special entry.
    /// Otherwise, the code assumes we are in identity-mapping, and
    /// the physical addresses in a table are used as virtual addresses.
    fn next_table_address(&self, index: usize, active: bool) -> Option<VirtualAddress> {
        let entry_flags = self[index].flags();
        // commenting until we understand how huge pages work on aarch64
        if entry_flags.contains(PteFlags::VALID) /*&& !entry_flags.is_huge()*/ {
            let table_address = self as *const _ as usize;
            let next_table_vaddr: usize = match active {

                // use the recursive entry
                true => (table_address << 9) | (index << PAGE_SHIFT),

                // use the identity mapping
                false => (self[index].value() & PAGE_TABLE_ENTRY_FRAME_MASK) as usize,

            };
            Some(VirtualAddress::new_canonical(next_table_vaddr))
        } else {
            None
        }
    }

    /// Returns a reference to the next lowest-level page table.
    /// 
    /// A convenience wrapper around `next_table_address()`; see that method for more.
    /// 
    /// With `active` set to true, you indicate that the
    /// modified table is currently used by the CPU, in which case
    /// it will be accessed using the recursive paging special entry.
    /// Otherwise, the code assumes we are in identity-mapping, and
    /// the physical addresses in a table are used as virtual addresses.
    pub fn next_table(&self, index: usize, active: bool) -> Option<&Table<L::NextLevel>> {
        // convert the next table address from a raw pointer back to a Table type
        self.next_table_address(index, active).map(|vaddr| unsafe { &*(vaddr.value() as *const _) })
    }

    /// Returns a mutable reference to the next lowest-level page table.
    /// 
    /// A convenience wrapper around `next_table_address()`; see that method for more.
    /// 
    /// With `active` set to true, you indicate that the
    /// modified table is currently used by the CPU, in which case
    /// it will be accessed using the recursive paging special entry.
    /// Otherwise, the code assumes we are in identity-mapping, and
    /// the physical addresses in a table are used as virtual addresses.
    pub fn next_table_mut(&mut self, index: usize, active: bool) -> Option<&mut Table<L::NextLevel>> {
        self.next_table_address(index, active).map(|vaddr| unsafe { &mut *(vaddr.value() as *mut _) })
    }

    /// Returns a mutable reference to the next lowest-level page table, 
    /// creating and initializing a new one if it doesn't already exist.
    /// 
    /// A convenience wrapper around `next_table_address()`; see that method for more.
    /// 
    /// With `active` set to true, you indicate that the
    /// modified table is currently used by the CPU, in which case
    /// it will be accessed using the recursive paging special entry.
    /// Otherwise, the code assumes we are in identity-mapping, and
    /// the physical addresses in a table are used as virtual addresses.
    ///
    /// TODO: return a `Result` here instead of panicking.
    pub fn next_table_create(
        &mut self,
        index: usize,
        flags: PteFlags,
        active: bool,
    ) -> Result<&mut Table<L::NextLevel>, &'static str> {
        if self.next_table(index, active).is_none() {
            // commenting until we understand how huge pages work on aarch64
            // assert!(!self[index].flags().is_huge(), "mapping code does not support huge pages");

            let af = frame_allocator::allocate_frames(1).ok_or("next_table_create(): no frames available")?;
            self[index].set_entry(af.as_allocated_frame(), flags.writable(true).valid(true));
            let table = self.next_table_mut(index, active).unwrap();
            table.zero();
            core::mem::forget(af); // we currently forget frames allocated as page table frames since we don't yet have a way to track them.
        }
        Ok(self.next_table_mut(index, active).unwrap())
    }
}

impl<L: TableLevel> Index<usize> for Table<L> {
    type Output = PageTableEntry;

    fn index(&self, index: usize) -> &PageTableEntry {
        &self.entries[index]
    }
}

impl<L: TableLevel> IndexMut<usize> for Table<L> {
    fn index_mut(&mut self, index: usize) -> &mut PageTableEntry {
        &mut self.entries[index]
    }
}

pub trait TableLevel {}

pub enum Level4 {}
#[allow(dead_code)]
pub enum Level3 {}
#[allow(dead_code)]
pub enum Level2 {}
pub enum Level1 {}

impl TableLevel for Level4 {}
impl TableLevel for Level3 {}
impl TableLevel for Level2 {}
impl TableLevel for Level1 {}

pub trait HierarchicalLevel: TableLevel {
    type NextLevel: TableLevel;
}

impl HierarchicalLevel for Level4 {
    type NextLevel = Level3;
}

impl HierarchicalLevel for Level3 {
    type NextLevel = Level2;
}

impl HierarchicalLevel for Level2 {
    type NextLevel = Level1;
}
