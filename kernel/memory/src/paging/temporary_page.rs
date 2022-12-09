// Copyright 2016 Philipp Oppermann. See the README.md
// file at the top-level directory of this distribution.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use core::mem::ManuallyDrop;
use log::error;
use super::{
    AllocatedPages, AllocatedFrames, PageTable, MappedPages, VirtualAddress,
    table::{Table, Level1, NextLevelAccess},
};
use pte_flags::PteFlagsArch;
use kernel_config::memory::{TEMPORARY_PAGE_VIRT_ADDR, PAGE_SIZE};
use owned_borrowed_trait::Owned;


/// A page that can be temporarily mapped to the recursive page table frame,
/// used for purposes of editing a top-level (P4) page table itself.
/// 
/// See how recursive paging works: <https://wiki.osdev.org/Page_Tables#Recursive_mapping>
///
/// # Note 
/// Instead of just dropping a `TemporaryPage` object, 
/// it should be cleaned up (reclaimed) using [`TemporaryPage::unmap_into_parts()`].

pub struct TemporaryPage {
    mapped_page: MappedPages,
    /// `ManuallyDrop` is required here in order to prevent this `AllocatedFrames` 
    /// from being dropped twice: once when unmapping the above `mapped_page`, and
    /// once when dropping this `TemporaryPage`.
    /// This is because the `AllocatedFrames` object here is the same one that is 
    /// mapped by the above `mapped_page`.
    frame: ManuallyDrop<AllocatedFrames>,
}

impl TemporaryPage {
    /// Creates a new [`TemporaryPage`] and maps it to the given page table `frame`
    /// in the active table.
    /// 
    /// # Arguments
    /// * `page`: the optional page that will be used for the temporary mapping.
    ///    If `None`, a new page will be allocated.
    /// * `frame`: the single frame containing the page table that we want to modify,
    ///    which will be mapped to this [`TemporaryPage`].
    /// * `page_table`: the currently active [`PageTable`].
    pub fn create_and_map_table_frame(
        mut page: Option<AllocatedPages>,
        frame: AllocatedFrames,
        page_table: &mut PageTable,
    ) -> Result<TemporaryPage, &'static str> {
        let mut vaddr = VirtualAddress::new_canonical(TEMPORARY_PAGE_VIRT_ADDR);
        while page.is_none() && vaddr.value() != 0 {
            page = page_allocator::allocate_pages_at(vaddr, 1).ok();
            vaddr -= PAGE_SIZE;
        }
        let (mapped_page, frame) = page_table.internal_map_to(
            page.ok_or("Couldn't allocate a new Page for the temporary P4 table frame")?,
            Owned(frame),
            PteFlagsArch::new().valid(true).writable(true),
            NextLevelAccess::Recursive,
        )?;
        Ok(TemporaryPage {
            mapped_page,
            frame: ManuallyDrop::new(frame),
        })
    }

    /// Invokes the given closure `f` with a mutable reference to the root P4 page table
    /// `Table` and `AllocatedFrame` held in this `TemporaryPage`.
    pub fn with_table_and_frame<F, R>(
        &mut self,
        f: F,
    ) -> Result<R, &'static str> 
        where F: FnOnce(&mut Table<Level1>, &AllocatedFrames) -> R
    {
        let res = f(
            self.mapped_page.as_type_mut(0)?,
            &self.frame,
        );
        Ok(res)
    }

    /// Call this to clean up a `TemporaryPage` instead of just letting it be dropped.
    ///
    /// A simple wrapper around [`MappedPages::unmap_into_parts()`].
    pub fn unmap_into_parts(mut self, page_table: &mut PageTable) -> Result<(AllocatedPages, Option<AllocatedFrames>), &'static str> {
        let mp = core::mem::replace(&mut self.mapped_page, MappedPages::empty());
        mp.unmap_into_parts(page_table).map_err(|e_mp| {
            error!("TemporaryPage::unmap_into_parts(): failed to unmap internal {:?}", e_mp);
            "BUG: TemporaryPage::unmap_into_parts(): failed to unmap internal MappedPages into its parts."
        })
    }
}
