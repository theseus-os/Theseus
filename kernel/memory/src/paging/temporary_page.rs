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
use super::{AllocatedPages, AllocatedFrames, FrameAllocator, VirtualAddress};
use kernel_config::memory::{TEMPORARY_PAGE_VIRT_ADDR, PAGE_SIZE};


/// A Page that can be temporarily mapped to the recursive page table frame,
/// used for purposes of editing a top-level (P4) PageTable itself.
/// 
/// See how recursive paging works: <https://wiki.osdev.org/Page_Tables#Recursive_mapping>
///
/// # Note 
/// Instead of just dropping a `TemporaryPage` object, it should be cleaned up using `unmap_into_parts()`.

pub struct TemporaryPage {
    mapped_page: Option<MappedPages>,
    allocator: TinyAllocator,
}

impl TemporaryPage {
    /// Creates a new [`TemporaryPage`] but does not yet map it to the recursive paging entry. 
    /// You must call [`map_table_frame()`](#method.map_table_frame) to do that. 
    pub fn new() -> TemporaryPage {
        TemporaryPage {
            mapped_page: None,
            allocator: TinyAllocator::new(),
        }
    }

    /// Maps the temporary page to the given page table frame in the active table.
    /// Returns a reference to the now mapped table.
    /// 
    /// # Arguments
    /// * `frame`: the [`Frame`] containing the page table that we want to modify, which will be mapped to this [`TemporaryPage`].     
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
                &mut self.allocator
            )?);
        }
        self.mapped_page.as_mut().unwrap().as_type_mut(0)
    }

    /// Call this to clean up a `TemporaryPage` instead of just letting it be dropped.
    /// A simple wrapper around `MappedPages::unmap_into_parts()`.
    pub fn unmap_into_parts(mut self, page_table: &mut PageTable) -> Result<(AllocatedPages, AllocatedFrames), &'static str> {
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
            warn!("BUG: TemporaryPage was dropped, should use `unmap_into_parts()` instead. Contained {:?}", _mp);
        }
    }    
}


use super::{FrameRange, Frame};

/// A simple wrapper around our real frame_allocator crate
/// that accommodates the `FrameAllocator` trait which allocates `Frame` objects.
/// FIXME: remove this once we use the real `frame_allocator` crate properly.
struct TinyAllocator;

impl TinyAllocator {
    fn new() -> TinyAllocator {
        TinyAllocator { }
    }
}

impl FrameAllocator for TinyAllocator {
    fn allocate_frame(&mut self) -> Option<Frame> {
        frame_allocator::allocate_frames(1).map(|af| {
            let frame = *af.start();
            // TODO: FIXME: properly handle allocated frames here once map_allocated_pages_to() actually works correctly.
            core::mem::forget(af); 
            frame
        })                    
    }

    
    fn allocate_frames(&mut self, _num_frames: usize) -> Option<FrameRange> {
        unimplemented!();
    }


    fn deallocate_frame(&mut self, frame: Frame) {
        todo!("Implement deallocate frames for TinyAllocator!");
    }

    fn alloc_ready(&mut self) {
        // this is a no-op
    }
}
