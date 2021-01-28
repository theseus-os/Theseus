// Copyright 2016 Philipp Oppermann. See the README.md
// file at the top-level directory of this distribution.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use {FrameRange};
use paging::{PageTable, MappedPages};
use super::table::{Table, Level1};
use super::{Frame, FrameAllocator, VirtualAddress};
use kernel_config::memory::{TEMPORARY_PAGE_VIRT_ADDR, PAGE_SIZE};


/// A Page that can be temporarily mapped to the recursive page table frame,
/// used for purposes of editing the page tables themselves. 
/// 
/// See how recursive paging works: <https://wiki.osdev.org/Page_Tables#Recursive_mapping>
pub struct TemporaryPage {
    mapped_page: Option<MappedPages>,
    allocator: TinyAllocator,
}

impl TemporaryPage {
    /// Creates a new [`TemporaryPage`] but does not yet map it to the recursive paging entry. 
    /// You must call [`map_table_frame()`](#method.map_table_frame) to do that. 
    /// 
    /// # Arguments 
    /// * `three_frames`: the three [`Frame`]s needed for the allocator contained within this [`TemporaryPage`]. 
    ///   To complete the recursive mapping to this temporary page, we may need to allocate at most 3 frames (for P1, P2, P3 table levels). 
    ///  
    pub fn new(three_frames: (Frame, Frame, Frame)) -> TemporaryPage {
        TemporaryPage {
            mapped_page: None,
            allocator: TinyAllocator::new(three_frames),
        }
    }


    /// Maps the temporary page to the given page table frame in the active table.
    /// Returns a reference to the now mapped table.
    /// 
    /// # Arguments
    /// * `frame`: the [`Frame`] containing the page table that we want to modify, which will be mapped to this [`TemporaryPage`].     
    /// * `page_table`: the currently active [`PageTable`]. 
    /// 
    pub fn map_table_frame(&mut self, frame: Frame, page_table: &mut PageTable) -> Result<&mut Table<Level1>, &'static str> {
        if self.mapped_page.is_none() {
            let mut vaddr = VirtualAddress::new_canonical(TEMPORARY_PAGE_VIRT_ADDR);
            let mut page = None;
            while page.is_none() && vaddr.value() != 0 {
                page = page_allocator::allocate_pages_at(vaddr, 1).ok();
                vaddr -= PAGE_SIZE;
            }
            self.mapped_page = Some(page_table.map_allocated_pages_to(
                page.ok_or("Couldn't allocate a new Page for the temporary P4 table frame")?,
                FrameRange::new(frame, frame),
                super::EntryFlags::WRITABLE,
                &mut self.allocator
            )?);
        }
        self.mapped_page.as_mut().unwrap().as_type_mut(0)
    }
}

struct TinyAllocator([Option<Frame>; 3]);

impl TinyAllocator {
    fn new(three_frames: (Frame, Frame, Frame)) -> TinyAllocator {
        let (f1, f2, f3) = three_frames;
        TinyAllocator( [Some(f1), Some(f2), Some(f3)] )
    }
}

impl FrameAllocator for TinyAllocator {
    fn allocate_frame(&mut self) -> Option<Frame> {
        for frame_option in &mut self.0 {
            if frame_option.is_some() {
                return frame_option.take();
            }
        }
        None
    }

    
    fn allocate_frames(&mut self, _num_frames: usize) -> Option<FrameRange> {
        unimplemented!();
    }


    fn deallocate_frame(&mut self, frame: Frame) {
        for frame_option in &mut self.0 {
            if frame_option.is_none() {
                *frame_option = Some(frame);
                return;
            }
        }
        error!("BUG: TinyAllocator::deallocate_frame(): deallocated too many frames, can hold only 3 frames.");
    }

    fn alloc_ready(&mut self) {
        // this is a no-op
    }
}

impl Drop for TinyAllocator {
    fn drop(&mut self) {
        // FIXME: TinyAllocator leaks 3 frames when it's dropped. 
        // Should call deallocate_frame() using the original allocator, which is returned by get_frame_allocator_ref()
    }
}
