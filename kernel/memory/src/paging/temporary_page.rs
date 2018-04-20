// Copyright 2016 Philipp Oppermann. See the README.md
// file at the top-level directory of this distribution.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use {FrameIter};
use paging::{ActivePageTable, MappedPages};
use super::table::{Table, Level1};
use super::{Page, Frame, FrameAllocator};
use kernel_config::memory::TEMPORARY_PAGE_VIRT_ADDR;


pub struct TemporaryPage {
    mapped_page: Option<MappedPages>,
    allocator: TinyAllocator,
}

impl TemporaryPage {
    pub fn new(three_frames: [Option<Frame>; 3]) -> TemporaryPage {
        TemporaryPage {
            mapped_page: None,
            allocator: TinyAllocator::new(three_frames),
        }
    }


    /// Maps the temporary page to the given page table frame in the active table.
    /// Returns a reference to the now mapped table.
    pub fn map_table_frame(&mut self, frame: Frame, active_table: &mut ActivePageTable) -> Result<&mut Table<Level1>, &'static str>
    {
        use super::entry::EntryFlags;

        // Find a free page that is not already mapped, starting from the top of the kernel heap region.
        // It'd be nice to use the virtual address allocator (allocate_pages), but we CANNOT use it
        // because this code is needed before those functions are available (cuz they require heap memory)
        let mut page = Page::containing_address(TEMPORARY_PAGE_VIRT_ADDR);
        while active_table.translate_page(page).is_some() {
            // this never happens
            warn!("temporary page {:?} is already mapped, trying the next lowest Page", page);
            page -= 1;
        }
        
        let mapped_page = try!(active_table.map_to(page, frame, EntryFlags::WRITABLE, &mut self.allocator));
        let vaddr = mapped_page.start_address();
        self.mapped_page = Some(mapped_page);

        unsafe { 
            Ok( &mut *(vaddr as *mut Table<Level1>) )
        }
    }

    // this is no longer needed now that we use the MappedPages type for auto-unmapping 
    // /// Unmaps the temporary page in the active table.
    // pub fn unmap(&mut self, active_table: &mut ActivePageTable) {
    //     active_table.unmap(self.mapped_page, &mut self.allocator)
    // }
}

struct TinyAllocator([Option<Frame>; 3]);

impl TinyAllocator {
    fn new(three_frames: [Option<Frame>; 3]) -> TinyAllocator {
        TinyAllocator(three_frames)
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

    
    fn allocate_frames(&mut self, _num_frames: usize) -> Option<FrameIter> {
        unimplemented!();
    }


    fn deallocate_frame(&mut self, frame: Frame) {
        for frame_option in &mut self.0 {
            if frame_option.is_none() {
                *frame_option = Some(frame);
                return;
            }
        }
        panic!("Tiny allocator can hold only 3 frames.");
    }

    fn alloc_ready(&mut self) {
        // this is a no-op
    }
}

impl Drop for TinyAllocator {
    fn drop(&mut self) {
        // FIXME: TinyAllocator leaks 3 frames when it's dropped. 
        // Should call deallocate_frame() using the original allocator, which is memory::FRAME_ALLOCATOR
    }
}
