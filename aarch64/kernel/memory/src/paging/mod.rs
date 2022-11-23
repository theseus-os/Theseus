// Copyright 2016 Philipp Oppermann. See the README.md
// file at the top-level directory of this distribution.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

mod temporary_page;
mod mapper;
#[cfg(not(mapper_spillful))]
mod table;
#[cfg(mapper_spillful)]
pub mod table;


pub use page_table_entry::PageTableEntry;
pub use self::{
    temporary_page::TemporaryPage,
    mapper::{
        Mapper, MappedPages, BorrowedMappedPages, BorrowedSliceMappedPages,
        Mutability, Mutable, Immutable,
    },
};

use core::{
    ops::{Deref, DerefMut},
    fmt,
};
use super::{Frame, FrameRange, PageRange, VirtualAddress, PhysicalAddress,
    AllocatedPages, allocate_pages, AllocatedFrames, PteFlags,
    tlb_flush_all, tlb_flush_virt_addr, get_p4, set_page_table_up};
use no_drop::NoDrop;
use kernel_config::memory::{RECURSIVE_P4_INDEX};
// use kernel_config::memory::{KERNEL_TEXT_P4_INDEX, KERNEL_HEAP_P4_INDEX, KERNEL_STACK_P4_INDEX};

#[cfg(target_arch = "x86_64")]
use super::{find_section_memory_bounds, get_vga_mem_addr};

/// A top-level root (P4) page table.
/// 
/// Auto-derefs into a `Mapper` for easy invocation of memory mapping functions.
pub struct PageTable {
    mapper: Mapper,
    p4_table: AllocatedFrames,
}
impl fmt::Debug for PageTable {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "PageTable(p4: {:#X})", self.p4_table.start_address()) 
    }
}

impl Deref for PageTable {
    type Target = Mapper;

    fn deref(&self) -> &Mapper {
        &self.mapper
    }
}

impl DerefMut for PageTable {
    fn deref_mut(&mut self) -> &mut Mapper {
        &mut self.mapper
    }
}

impl PageTable {
    /// An internal function to bootstrap a new top-level PageTable 
    /// based on the given currently-active P4 frame (the frame holding the page table root).
    /// 
    /// Returns an error if the given `active_p4_frame` is not the currently active page table.
    fn from_current(active_p4_frame: AllocatedFrames) -> Result<PageTable, &'static str> {
        assert!(active_p4_frame.size_in_frames() == 1);
        let current_p4 = get_current_p4();
        if active_p4_frame.start() != &current_p4 {
            return Err("PageTable::from_current(): the active_p4_frame must be the root of the currently-active page table.");
        }
        Ok(PageTable { 
            mapper: Mapper::with_p4_frame(current_p4),
            p4_table: active_p4_frame,
        })
    }

    /// Initializes a new top-level P4 `PageTable` whose root is located in the given `new_p4_frame`.
    /// It requires using the given `current_active_table` to set up its initial mapping contents.
    /// 
    /// A single allocated page can optionally be provided for use as part of a new `TemporaryPage`
    /// for the recursive mapping.
    /// 
    /// Returns the new `PageTable` that exists in physical memory at the given `new_p4_frame`. 
    /// Note that this new page table has no current mappings beyond the recursive P4 mapping,
    /// so you will need to create or copy over any relevant mappings 
    /// before using (switching) to this new page table in order to ensure the system keeps running.
    pub fn new_table(
        current_page_table: &mut PageTable,
        new_p4_frame: AllocatedFrames,
        page: Option<AllocatedPages>,
    ) -> Result<PageTable, &'static str> {
        let p4_frame = *new_p4_frame.start();

        let mut temporary_page = TemporaryPage::create_and_map_table_frame(page, new_p4_frame, current_page_table)?;
        temporary_page.with_table_and_frame(|table, frame| {
            table.zero();
            table[RECURSIVE_P4_INDEX].set_entry(frame.as_allocated_frame(), PteFlags::VALID | PteFlags::WRITABLE);
        })?;

        let (_temp_page, inited_new_p4_frame) = temporary_page.unmap_into_parts(current_page_table)?;

        Ok(PageTable {
            mapper: Mapper::with_p4_frame(p4_frame),
            p4_table: inited_new_p4_frame.ok_or("BUG: PageTable::new_table(): failed to take back unmapped Frame for p4_table")?,
        })
    }

    /// Temporarily maps the given other `PageTable` to the recursive entry (510th entry) 
    /// so that the given closure `f` can set up new mappings on the new `other_table` without actually switching to it yet.
    /// Accepts a closure `f` that is passed  a `Mapper`, such that it can set up new mappings on the other table.
    /// Consumes the given `temporary_page` and automatically unmaps it afterwards. 
    /// # Note
    /// This does not perform any task switching or changing of the current page table register (e.g., cr3).
    pub fn with<F>(
        &mut self,
        other_table: &mut PageTable,
        f: F,
    ) -> Result<(), &'static str>
        where F: FnOnce(&mut Mapper) -> Result<(), &'static str>
    {
        let active_p4_frame = get_current_p4();
        if self.p4_table.start() != &active_p4_frame || self.p4_table.end() != &active_p4_frame {
            return Err("PageTable::with(): this PageTable ('self') must be the currently active page table.");
        }

        // Temporarily take ownership of this page table's p4 allocated frame and
        // create a new temporary page that maps to that frame.
        let this_p4 = core::mem::replace(&mut self.p4_table, AllocatedFrames::empty());
        let mut temporary_page = TemporaryPage::create_and_map_table_frame(None, this_p4, self)?;

        // overwrite recursive mapping
        let p4_flags = PteFlags::VALID | PteFlags::WRITABLE | PteFlags::ACCESSED;
        self.p4_mut()[RECURSIVE_P4_INDEX].set_entry(other_table.p4_table.as_allocated_frame(), p4_flags); 
        tlb_flush_all();

        // set mapper's target frame to reflect that future mappings will be mapped into the other_table
        self.mapper.target_p4 = *other_table.p4_table.start();

        // execute `f` in the new context, in which the new page table is considered "active"
        let ret = f(self);

        // restore mapper's target frame to reflect that future mappings will be mapped using the currently-active (original) PageTable
        self.mapper.target_p4 = active_p4_frame;

        // restore recursive mapping to original p4 table
        temporary_page.with_table_and_frame(|p4_table, frame| {
            p4_table[RECURSIVE_P4_INDEX].set_entry(frame.as_allocated_frame(), PteFlags::VALID | PteFlags::WRITABLE);
        })?;
        tlb_flush_all();

        // Here, recover the current page table's p4 frame and restore it into this current page table,
        // since we removed it earlier at the top of this function and gave it to the temporary page. 
        let (_temp_page, p4_frame) = temporary_page.unmap_into_parts(self)?;
        self.p4_table = p4_frame.ok_or("BUG: PageTable::with(): failed to take back unmapped Frame for p4_table")?;

        ret
    }


    /// Switches from the currently-active page table (this `PageTable`, i.e., `self`) to the given `new_table`.
    /// After this function, the given `new_table` will be the currently-active `PageTable`.
    pub fn switch(&mut self, new_table: &PageTable) {
        // debug!("PageTable::switch() old table: {:?}, new table: {:?}", self, new_table);

        // perform the actual page table switch
        #[cfg(target_arch = "x86_64")]
        unsafe {
            use x86_64::{PhysAddr, structures::paging::frame::PhysFrame, registers::control::{Cr3, Cr3Flags}};
            Cr3::write(
                PhysFrame::containing_address(PhysAddr::new_truncate(new_table.p4_table.start_address().value() as u64)),
                Cr3Flags::empty(),
            )
        };

        #[cfg(target_arch = "aarch64")]
        set_page_table_up(new_table.physical_address());
    }


    /// Returns the physical address of this page table's top-level p4 frame
    pub fn physical_address(&self) -> PhysicalAddress {
        self.p4_table.start_address()
    }
}


/// Returns the current top-level (P4) root page table frame.
pub fn get_current_p4() -> Frame {
    Frame::containing_address(get_p4())
}

/// Initializes a new page table and sets up all necessary mappings for the kernel to continue running. 
/// Returns the kernel's current PageTable, if successful.
/// Otherwise, it returns a str error message. 
pub fn init(into_alloc_frames_fn: fn(FrameRange) -> AllocatedFrames) -> Result<PageTable, &'static str> {
    // Store the callback from `frame_allocator::init()` that allows the `Mapper` to convert
    // `page_table_entry::UnmappedFrames` back into `AllocatedFrames`.
    mapper::INTO_ALLOCATED_FRAMES_FUNC.call_once(|| into_alloc_frames_fn);

    // bootstrap a PageTable from the currently-loaded page table
    let current_p4 = get_current_p4().start_address();
    let current_active_p4 = frame_allocator::allocate_frames_at(current_p4, 1)?;
    let current_page_table = PageTable::from_current(current_active_p4)?;
    debug!("Bootstrapped initial {:?}", current_page_table);

    // todo: build new page table and switch to it

    Ok(current_page_table)
}
