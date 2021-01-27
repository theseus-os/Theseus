// Copyright 2016 Philipp Oppermann. See the README.md
// file at the top-level directory of this distribution.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

mod entry;
mod temporary_page;
mod mapper;
#[cfg(not(mapper_spillful))]
mod table;
#[cfg(mapper_spillful)]
pub mod table;


pub use self::entry::*;
pub use self::temporary_page::TemporaryPage;
pub use self::mapper::*;

use core::{
    ops::{Deref, DerefMut},
    fmt,
};
use super::*;

use kernel_config::memory::{RECURSIVE_P4_INDEX};
// use kernel_config::memory::{KERNEL_TEXT_P4_INDEX, KERNEL_HEAP_P4_INDEX, KERNEL_STACK_P4_INDEX};


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
    /// An internal function to create a new top-level PageTable 
    /// based on the currently-active page table register (e.g., CR3). 
    ///
    /// FIXME: remove this function, it's no longer safe now that frame deallocation has been reworked.
    #[deprecated(since = "2021-01-26", note = "no longer safe now that frame deallocation has been reworked")]
    fn from_current() -> PageTable {
        let current_p4 = get_current_p4();
        let af = unsafe { AllocatedFrames::from_parts_unsafe(FrameRange::new(current_p4, current_p4)) };
        PageTable { 
            mapper: Mapper::from_current(),
            p4_table: af,
        }
    }

    /// Initializes a brand new top-level P4 `PageTable` whose root is located in the given `new_p4_frame`. 
    /// It requires using the given `current_active_table` to set up its initial mapping contents,
    /// for which the given `TemporaryPage` is used to recursively map it (and is auto-unmapped upon return). 
    /// 
    /// Returns the new `PageTable` that exists in physical memory at the given `new_p4_frame`. 
    /// Note that this new page table has no current mappings beyond the recursive P4 mapping,
    /// so you will need to create or copy over any relevant mappings 
    /// before using (switching) to this new page table in order to ensure the system keeps running.
    pub fn new_table(
        current_page_table: &mut PageTable,
        new_p4_frame: AllocatedFrames,
        mut temporary_page: TemporaryPage,
    ) -> Result<PageTable, &'static str> {
        assert!(new_p4_frame.size_in_frames() == 1);
        let p4_frame = new_p4_frame.start().clone();
        
        {
            let table = temporary_page.map_table_frame(new_p4_frame, current_page_table)?;
            table.zero();

            table[RECURSIVE_P4_INDEX].set(p4_frame, EntryFlags::PRESENT | EntryFlags::WRITABLE);

            // Note: now that virtual pages are all dynamically allocated,
            //       we don't want to copy any mappings by default. 
            //
            // // start out by copying all the kernel sections into the new table
            // table.copy_entry_from_table(current_page_table.p4(), KERNEL_TEXT_P4_INDEX);
            // table.copy_entry_from_table(current_page_table.p4(), KERNEL_HEAP_P4_INDEX);
            // table.copy_entry_from_table(current_page_table.p4(), KERNEL_STACK_P4_INDEX);
        }

        let (_temp_page, inited_new_p4_frame) = temporary_page.unmap_into_parts(current_page_table)?;
        warn!("PageTable::new_table(): _temp_page: {:?}, inited_new_p4_frame: {:?}", _temp_page, inited_new_p4_frame);

        Ok( PageTable { 
            mapper: Mapper::with_p4_frame(p4_frame),
            p4_table: inited_new_p4_frame,
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
        mut temporary_page: temporary_page::TemporaryPage,
        f: F,
    ) -> Result<(), &'static str>
        where F: FnOnce(&mut Mapper) -> Result<(), &'static str>
    {
        let active_p4_frame = get_current_p4();
        if self.p4_table.start() != &active_p4_frame || self.p4_table.end() != &active_p4_frame {
            return Err("PageTable::with(): this PageTable ('self') must be the currently active page table.");
        }

        // Temporarily take ownership of the p4 allocated frame for this page table.
        let this_p4 = core::mem::replace(&mut self.p4_table, AllocatedFrames::empty());

        // map temporary_page to current p4 table
        let p4_table = temporary_page.map_table_frame(this_p4, self)?;

        // overwrite recursive mapping
        self.p4_mut()[RECURSIVE_P4_INDEX].set(*other_table.p4_table.start(), EntryFlags::PRESENT | EntryFlags::WRITABLE); 
        tlb_flush_all();

        // set mapper's target frame to reflect that future mappings will be mapped into the other_table
        self.mapper.target_p4 = *other_table.p4_table.start();

        // execute `f` in the new context, in which the new page table is considered "active"
        let ret = f(self);

        // restore mapper's target frame to reflect that future mappings will be mapped using the currently-active (original) PageTable
        self.mapper.target_p4 = active_p4_frame;

        // restore recursive mapping to original p4 table
        p4_table[RECURSIVE_P4_INDEX].set(active_p4_frame, EntryFlags::PRESENT | EntryFlags::WRITABLE);
        tlb_flush_all();

        // Here, recover the current page table's p4 frame and restore it into this current page table,
        // since we removed it earlier at the top of this function and gave it to the temporary page. 
        let (_temp_page, p4_frame) = temporary_page.unmap_into_parts(self)?;
        self.p4_table = p4_frame;

        ret
    }


    /// Switches from the currently-active page table (this `PageTable`, i.e., `self`) to the given `new_table`.
    /// Returns the newly-switched-to PageTable.
    pub fn switch(&mut self, new_table: &PageTable) -> PageTable {
        // debug!("PageTable::switch() old table: {:?}, new table: {:?}", self, new_table);

        // perform the actual page table switch
        unsafe { x86_64::registers::control_regs::cr3_write(x86_64::PhysicalAddress(new_table.p4_table.start_address().value() as u64)) };
        let current_table_after_switch = PageTable::from_current();
        current_table_after_switch
    }


    /// Returns the physical address of this page table's top-level p4 frame
    pub fn physical_address(&self) -> PhysicalAddress {
        self.p4_table.start_address()
    }
}


/// Returns the current top-level page table frame
pub fn get_current_p4() -> Frame {
    Frame::containing_address(get_p4())
}


/// Initializes a new page table and sets up all necessary mappings for the kernel to continue running. 
/// Returns the following tuple, if successful:
/// 
///  * The kernel's new PageTable, which is now currently active,
///  * the kernels' text section MappedPages,
///  * the kernels' rodata section MappedPages,
///  * the kernels' data section MappedPages,
///  * a tuple of the stack's underlying guard page (an `AllocatedPages` instance) and the actual `MappedPages` backing it,
///  * the kernel's list of *other* higher-half MappedPages that needs to be converted to a vector after heap initialization, and which should be kept forever,
///  * the kernel's list of identity-mapped MappedPages that needs to be converted to a vector after heap initialization, and which should be dropped before starting the first userspace program. 
///
/// Otherwise, it returns a str error message. 
pub fn init(
    boot_info: &multiboot2::BootInformation,
) -> Result<(
        PageTable,
        MappedPages,
        MappedPages,
        MappedPages,
        (AllocatedPages, MappedPages),
        [Option<MappedPages>; 32],
        [Option<MappedPages>; 32]
    ), &'static str>
{
    // bootstrap a PageTable from the currently-loaded page table
    let mut page_table = PageTable::from_current();

    let boot_info_start_vaddr = VirtualAddress::new(boot_info.start_address()).map_err(|_| "boot_info start virtual address was invalid")?;
    let boot_info_start_paddr = page_table.translate(boot_info_start_vaddr).ok_or("Couldn't get boot_info start physical address")?;
    let boot_info_size = boot_info.total_size();
    info!("multiboot vaddr: {:#X}, multiboot paddr: {:#X}, size: {:#X}\n", boot_info_start_vaddr, boot_info_start_paddr, boot_info_size);

    let new_p4_frame = frame_allocator::allocate_frames(1).ok_or("couldn't allocate frame for new page table")?; 
    warn!("new_p4_frame: {:?}", new_p4_frame);
    let mut new_table = PageTable::new_table(&mut page_table, new_p4_frame, TemporaryPage::new())?;
    warn!("new_table: {:?}", new_table);

    let mut text_mapped_pages: Option<MappedPages> = None;
    let mut rodata_mapped_pages: Option<MappedPages> = None;
    let mut data_mapped_pages: Option<MappedPages> = None;
    let mut stack_pages: Option<(AllocatedPages, MappedPages)> = None;
    let mut higher_half_mapped_pages: [Option<MappedPages>; 32] = Default::default();
    let mut identity_mapped_pages: [Option<MappedPages>; 32] = Default::default();

    // consumes and auto unmaps temporary page
    page_table.with(&mut new_table, TemporaryPage::new(), |mapper| {

        let (aggregated_section_memory_bounds, sections_memory_bounds) = find_section_memory_bounds(&boot_info)?;
        
        // Map every section found in the kernel image (given by boot information above) into memory. 
        // To allow the APs to boot up, we identity map those kernel sections too
        // (lower half virtual addresses mapped to same lower half physical addresses).
        // We will unmap these lower-half identity mappings later, before we start running applications.
        let mut index = 0;
        for sec in sections_memory_bounds.iter().filter_map(|s| s.as_ref()).fuse() {
            let (start_virt_addr, start_phys_addr) = sec.start;
            let (_end_virt_addr, end_phys_addr) = sec.end;
            let size = end_phys_addr.value() - start_phys_addr.value();
            // let frames = FrameRange::from_phys_addr(start_phys_addr, size);
            let frames = frame_allocator::allocate_frames_by_bytes_at(start_phys_addr, size)?;
            let pages = page_allocator::allocate_pages_at(start_virt_addr - KERNEL_OFFSET, frames.size_in_frames())?;
            identity_mapped_pages[index] = Some(
                mapper.map_allocated_pages_to(
                    pages,
                    frames,
                    sec.flags,
                )?
            );
            debug!("           also mapped vaddr {:#X} to paddr {:#x} (size {:#X})", start_virt_addr - KERNEL_OFFSET, start_phys_addr, size);
            index += 1;
        }


        let (text_start_virt,    text_start_phys)    = aggregated_section_memory_bounds.text.start;
        let (text_end_virt,      text_end_phys)      = aggregated_section_memory_bounds.text.end;
        let (rodata_start_virt,  rodata_start_phys)  = aggregated_section_memory_bounds.rodata.start;
        let (rodata_end_virt,    rodata_end_phys)    = aggregated_section_memory_bounds.rodata.end;
        let (data_start_virt,    data_start_phys)    = aggregated_section_memory_bounds.data.start;
        let (data_end_virt,      data_end_phys)      = aggregated_section_memory_bounds.data.end;
        let (stack_start_virt,   stack_start_phys)   = aggregated_section_memory_bounds.stack.start;
        let (stack_end_virt,     _stack_end_phys)    = aggregated_section_memory_bounds.stack.end;

        let text_flags    = aggregated_section_memory_bounds.text.flags;
        let rodata_flags  = aggregated_section_memory_bounds.rodata.flags;
        let data_flags    = aggregated_section_memory_bounds.data.flags;


        // Map all the main kernel sections: text, rodata, and data.
        text_mapped_pages = Some(mapper.map_allocated_pages_to(
            page_allocator::allocate_pages_by_bytes_at(text_start_virt, text_end_virt.value() - text_start_virt.value())?, 
            // FrameRange::from_phys_addr(text_start_phys, text_end_phys.value() - text_start_phys.value()), 
            frame_allocator::allocate_frames_by_bytes_at(text_start_phys, text_end_phys.value() - text_start_phys.value())?,
            text_flags,
        )?);
        rodata_mapped_pages = Some(mapper.map_allocated_pages_to(
            page_allocator::allocate_pages_by_bytes_at(rodata_start_virt, rodata_end_virt.value() - rodata_start_virt.value())?, 
            // FrameRange::from_phys_addr(rodata_start_phys, rodata_end_phys.value() - rodata_start_phys.value()), 
            frame_allocator::allocate_frames_by_bytes_at(rodata_start_phys, rodata_end_phys.value() - rodata_start_phys.value())?,
            rodata_flags,
        )?);
        data_mapped_pages = Some(mapper.map_allocated_pages_to(
            page_allocator::allocate_pages_by_bytes_at(data_start_virt, data_end_virt.value() - data_start_virt.value())?, 
            // FrameRange::from_phys_addr(data_start_phys, data_end_phys.value() - data_start_phys.value()),
            frame_allocator::allocate_frames_by_bytes_at(data_start_phys, data_end_phys.value() - data_start_phys.value())?,
            data_flags,
        )?);

        // Handle the stack (a separate data section), which has one guard page followed by the real stack pages.
        let pages = page_allocator::allocate_pages_by_bytes_at(stack_start_virt, (stack_end_virt - stack_start_virt).value())?;
        let start_of_stack_pages = *pages.start() + 1; 
        let (stack_guard_page, stack_allocated_pages) = pages.split(start_of_stack_pages)
            .map_err(|_ap| "BUG: initial stack's allocated pages were not split correctly after guard page")?;
        let stack_start_frame = Frame::containing_address(stack_start_phys) + 1; // skip 1st frame, which corresponds to the guard page
        let stack_allocated_frames = frame_allocator::allocate_frames_at(stack_start_frame.start_address(), stack_allocated_pages.size_in_pages())?;
        let stack_mapped_pages = mapper.map_allocated_pages_to(
            stack_allocated_pages,
            stack_allocated_frames,
            data_flags,
        )?;
        stack_pages = Some((stack_guard_page, stack_mapped_pages));

        // map the VGA display memory as writable
        let (vga_display_phys_addr, vga_size_in_bytes, vga_display_flags) = get_vga_mem_addr()?;
        let vga_display_virt_addr = VirtualAddress::new_canonical(vga_display_phys_addr.value() + KERNEL_OFFSET);
        higher_half_mapped_pages[index] = Some(mapper.map_allocated_pages_to(
            page_allocator::allocate_pages_by_bytes_at(vga_display_virt_addr, vga_size_in_bytes)?, 
            // FrameRange::from_phys_addr(vga_display_phys_addr, vga_size_in_bytes), 
            frame_allocator::allocate_frames_by_bytes_at(vga_display_phys_addr, vga_size_in_bytes)?, 
            vga_display_flags,
        )?);
        debug!("mapped kernel section: vga_buffer at addr: {:#X} and {:#X}, size {} bytes", 
            vga_display_virt_addr, vga_display_virt_addr - KERNEL_OFFSET, vga_size_in_bytes
        );
        // also do an identity mapping for APs that need it while booting
        identity_mapped_pages[index] = Some(mapper.map_allocated_pages_to(
            page_allocator::allocate_pages_by_bytes_at(vga_display_virt_addr - KERNEL_OFFSET, vga_size_in_bytes)?, 
            // FrameRange::from_phys_addr(vga_display_phys_addr, vga_size_in_bytes), 
            frame_allocator::allocate_frames_by_bytes_at(vga_display_phys_addr, vga_size_in_bytes)?, 
            vga_display_flags,
        )?);
        index += 1;
        

        // map the multiboot boot_info at the same address it is currently at, so we can continue to access boot_info 
        let boot_info_pages  = PageRange::from_virt_addr(boot_info_start_vaddr, boot_info_size);
        debug!("Boot info covers pages: {:?}", boot_info_pages);
        // let boot_info_frames = FrameRange::from_phys_addr(boot_info_start_paddr, boot_info_size);
        let boot_info_frames = frame_allocator::allocate_frames_by_bytes_at(boot_info_start_paddr, boot_info_size)?;
        let boot_info_pages = page_allocator::allocate_pages_by_bytes_at(boot_info_start_vaddr, boot_info_size)?;
        debug!("Mapping boot info pages {:?} to frames {:?}", boot_info_pages, boot_info_frames);
        higher_half_mapped_pages[index] = Some(mapper.map_allocated_pages_to(
            boot_info_pages, boot_info_frames, EntryFlags::PRESENT | EntryFlags::GLOBAL,
        )?);
        index += 1;

        debug!("identity_mapped_pages: {:?}", &identity_mapped_pages[0..=index]);

        Ok(()) // mapping closure completed successfully

    })?; // TemporaryPage is dropped here


    let text_mapped_pages   = text_mapped_pages  .ok_or("Couldn't map .text section")?;
    let rodata_mapped_pages = rodata_mapped_pages.ok_or("Couldn't map .rodata section")?;
    let data_mapped_pages   = data_mapped_pages  .ok_or("Couldn't map .data section")?;
    let stack_pages         = stack_pages        .ok_or("Couldn't map .stack section")?;

    debug!("switching to new page table {:?}", new_table);
    let new_page_table = page_table.switch(&new_table); 
    // here, new_page_table and new_table should be identical
    debug!("switched to new page table {:?}.", new_page_table); 

    // Return the new_page_table because that's the one that should be used by the kernel in future mappings. 
    Ok((
        new_page_table,
        text_mapped_pages,
        rodata_mapped_pages,
        data_mapped_pages,
        stack_pages,
        higher_half_mapped_pages,
        identity_mapped_pages
    ))
}

