// Copyright 2016 Philipp Oppermann. See the README.md
// file at the top-level directory of this distribution.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use core::ops::{Index, IndexMut};
use core::marker::PhantomData;
use super::PageTableEntry;
use crate::VirtualAddress;
use pte_flags::PteFlagsArch;
use kernel_config::memory::{PAGE_SHIFT, ENTRIES_PER_PAGE_TABLE};
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

pub const TEMP_P4: *mut Table<Level4> = 0o177777_772_772_772_772_0000 as *mut _; 

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
    fn next_table_address(&self, index: usize) -> Option<VirtualAddress> {
        let pte_flags = self[index].flags();
        if pte_flags.is_valid() && !pte_flags.is_huge() {
            let table_address = self as *const _ as usize;
            let next_table_vaddr: usize = (table_address << 9) | (index << PAGE_SHIFT);
            Some(VirtualAddress::new_canonical(next_table_vaddr))
        } else {
            None
        }
    }

    /// Returns a reference to the next lowest-level page table.
    /// 
    /// A convenience wrapper around `next_table_address()`; see that method for more.
    pub fn next_table(&self, index: usize) -> Option<&Table<L::NextLevel>> {
        // convert the next table address from a raw pointer back to a Table type
        self.next_table_address(index).map(|vaddr| unsafe { &*(vaddr.value() as *const _) })
    }

    /// Returns a mutable reference to the next lowest-level page table.
    /// 
    /// A convenience wrapper around `next_table_address()`; see that method for more.
    pub fn next_table_mut(&mut self, index: usize) -> Option<&mut Table<L::NextLevel>> {
        self.next_table_address(index).map(|vaddr| unsafe { &mut *(vaddr.value() as *mut _) })
    }

    /// Returns a mutable reference to the next lowest-level page table, 
    /// creating and initializing a new one if it doesn't already exist.
    /// 
    /// A convenience wrapper around `next_table_address()`; see that method for more.
    ///
    /// TODO: return a `Result` here instead of panicking.
    pub fn next_table_create(
        &mut self,
        index: usize,
        flags: PteFlagsArch,
    ) -> &mut Table<L::NextLevel> {
        // log::debug!("NEXT TABLE CREATE");
        if self.next_table(index).is_none() {
            // log::debug!("creating next table");
            assert!(!self[index].flags().is_huge(), "mapping code does not support huge pages");
            // log::debug!("allocating frames");
            let af = frame_allocator::allocate_frames(1).expect("next_table_create(): no frames available");
            // log::debug!("setting entry");
            self[index].set_entry(
                af.as_allocated_frame(),
                flags.valid(true).writable(true), // must be valid and writable on x86_64
            );
            // log::debug!("zeroing next table");
            self.next_table_mut(index).unwrap().zero();
            core::mem::forget(af); // we currently forget frames allocated as page table frames since we don't yet have a way to track them.
        }
        // log::debug!("getting next table");
        self.next_table_mut(index).unwrap()
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
