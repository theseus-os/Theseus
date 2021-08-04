// Copyright 2016 Philipp Oppermann. See the README.md
// file at the top-level directory of this distribution.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use paging::{PageTable, MappedPages};
use super::table::{Table, Level1};
use super::{AllocatedPages, AllocatedFrames, VirtualAddress};
use kernel_config::memory::{TEMPORARY_PAGE_VIRT_ADDR, PAGE_SIZE};


/// A page that can be temporarily mapped to the recursive page table frame,
/// used for purposes of editing a top-level (P4) page table itself.
/// 
/// See how recursive paging works: <https://wiki.osdev.org/Page_Tables#Recursive_mapping>
///
/// # Note 
/// Instead of just dropping a `TemporaryPage` object, 
/// it should be cleaned up (reclaimed) using [`TemporaryPage::unmap_into_parts()`].

pub struct TemporaryPage {
    mapped_page: Option<MappedPages>,
}

impl TemporaryPage {
    /// Creates a new [`TemporaryPage`] but does not yet map it to the recursive paging entry. 
    /// You must call [`map_table_frame()`](#method.map_table_frame) to do that. 
    pub fn new() -> TemporaryPage {
        TemporaryPage {
            mapped_page: None,
        }
    }

    /// Maps the temporary page to the given page table frame in the active table.
    /// Returns a reference to the now mapped table.
    /// 
    /// # Arguments
    /// * `frame`: the single frame containing the page table that we want to modify, which will be mapped to this [`TemporaryPage`].
    /// * `page_table`: the currently active [`PageTable`]. 
    /// 
    pub fn map_table_frame(&mut self, frame: AllocatedFrames, page_table: &mut PageTable) -> Result<&mut Table<Level1>, &'static str> {
        if self.mapped_page.is_none() {
            let mut vaddr = VirtualAddress::new_canonical(TEMPORARY_PAGE_VIRT_ADDR);
            let mut page = None;
            while page.is_none() && vaddr.value() != 0 {
                page = page_allocator::allocate_pages_at(vaddr, 1).ok();
                vaddr -= PAGE_SIZE;
            }
            self.mapped_page = Some(page_table.map_allocated_pages_to(
                page.ok_or("Couldn't allocate a new Page for the temporary P4 table frame")?,
                frame,
                super::EntryFlags::WRITABLE,
            )?);
        }
        self.mapped_page.as_mut().unwrap().as_type_mut(0)
    }

    /// Call this to clean up a `TemporaryPage` instead of just letting it be dropped.
    /// A simple wrapper around [`MappedPages::unmap_into_parts()`].
    ///
    /// This is useful for unmapping pages but still maintaining ownership of the previously-mapped pages and frames
    /// without having them be auto-dropped as normal.
    pub fn unmap_into_parts(mut self, page_table: &mut PageTable) -> Result<(AllocatedPages, Option<AllocatedFrames>), &'static str> {
        if let Some(mp) = self.mapped_page.take() {
            mp.unmap_into_parts(page_table).map_err(|e_mp| {
                error!("TemporaryPage::unmap_into_parts(): failed to unmap internal {:?}", e_mp);
                "BUG: TemporaryPage::unmap_into_parts(): failed to unmap internal MappedPages into its parts."
            })
        } else {
            Err("BUG: TemporaryPage::unmap_into_parts(): temporary page had no mapped_page in it yet.")
        }
    }
}

impl Drop for TemporaryPage {
    fn drop(&mut self) {
        if let Some(_mp) = self.mapped_page.take() {
            error!("LIKELY BUG: TemporaryPage was dropped, should use `unmap_into_parts()` instead. Contained {:?}", _mp);
        }
    }    
}
