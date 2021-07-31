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


pub use page_table_entry::*;
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
            table[RECURSIVE_P4_INDEX].set_entry(p4_frame, EntryFlags::PRESENT | EntryFlags::WRITABLE);
        }

        let (_temp_page, inited_new_p4_frame) = temporary_page.unmap_into_parts(current_page_table)?;

        Ok( PageTable { 
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
        self.p4_mut()[RECURSIVE_P4_INDEX].set_entry(*other_table.p4_table.start(), EntryFlags::PRESENT | EntryFlags::WRITABLE); 
        tlb_flush_all();

        // set mapper's target frame to reflect that future mappings will be mapped into the other_table
        self.mapper.target_p4 = *other_table.p4_table.start();

        // execute `f` in the new context, in which the new page table is considered "active"
        let ret = f(self);

        // restore mapper's target frame to reflect that future mappings will be mapped using the currently-active (original) PageTable
        self.mapper.target_p4 = active_p4_frame;

        // restore recursive mapping to original p4 table
        p4_table[RECURSIVE_P4_INDEX].set_entry(active_p4_frame, EntryFlags::PRESENT | EntryFlags::WRITABLE);
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
        unsafe { 
            x86_64::registers::control_regs::cr3_write(
                x86_64::PhysicalAddress(new_table.p4_table.start_address().value() as u64)
            )
        };
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
/// Returns the following tuple, if successful:
/// 
///  1. The kernel's new PageTable, which is now currently active,
///  2. the kernels' text section MappedPages,
///  3. the kernels' rodata section MappedPages,
///  4. the kernels' data section MappedPages,
///  5. a tuple of the stack's underlying guard page (an `AllocatedPages` instance) and the actual `MappedPages` backing it,
///  6. the `MappedPages` holding the bootloader info,
///  7. the kernel's list of *other* higher-half MappedPages that needs to be converted to a vector after heap initialization, and which should be kept forever,
///  8. the kernel's list of identity-mapped MappedPages that needs to be converted to a vector after heap initialization, and which should be dropped before starting the first userspace program. 
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
        MappedPages,
        [Option<MappedPages>; 32],
        [Option<MappedPages>; 32]
    ), &'static str>
{
    let (aggregated_section_memory_bounds, _sections_memory_bounds) = find_section_memory_bounds(boot_info)?;
    debug!("{:X?}\n{:X?}", aggregated_section_memory_bounds, _sections_memory_bounds);
    
    // bootstrap a PageTable from the currently-loaded page table
    let current_active_p4 = frame_allocator::allocate_frames_at(aggregated_section_memory_bounds.page_table.start.1, 1)?;
    let mut page_table = PageTable::from_current(current_active_p4)?;
    debug!("Bootstrapped initial {:?}", page_table);

    let boot_info_start_vaddr = VirtualAddress::new(boot_info.start_address()).ok_or("boot_info start virtual address was invalid")?;
    let boot_info_start_paddr = page_table.translate(boot_info_start_vaddr).ok_or("Couldn't get boot_info start physical address")?;
    let boot_info_size = boot_info.total_size();
    debug!("multiboot vaddr: {:#X}, multiboot paddr: {:#X}, size: {:#X}\n", boot_info_start_vaddr, boot_info_start_paddr, boot_info_size);

    let new_p4_frame = frame_allocator::allocate_frames(1).ok_or("couldn't allocate frame for new page table")?; 
    let mut new_table = PageTable::new_table(&mut page_table, new_p4_frame, TemporaryPage::new())?;

    let mut text_mapped_pages:        Option<MappedPages> = None;
    let mut rodata_mapped_pages:      Option<MappedPages> = None;
    let mut data_mapped_pages:        Option<MappedPages> = None;
    let mut stack_page_group:         Option<(AllocatedPages, MappedPages)> = None;
    let mut boot_info_mapped_pages:   Option<MappedPages> = None;
    let mut higher_half_mapped_pages: [Option<MappedPages>; 32] = Default::default();
    let mut identity_mapped_pages:    [Option<MappedPages>; 32] = Default::default();

    // Create and initialize a new page table with the same contents as the currently-executing kernel code/data sections.
    page_table.with(&mut new_table, TemporaryPage::new(), |mapper| {
        
        // Map every section found in the kernel image (given by the boot information above) into our new page table. 
        // To allow the APs to boot up, we must identity map those kernel sections too, i.e., 
        // map the same physical frames to both lower-half and higher-half virtual addresses. 
        // This is the only time in Theseus that we permit non-bijective (non 1-to-1) virtual-to-physical memory mappings,
        // since it is unavoidable if we want to place the kernel in the higher half. 
        // Debatably, this is no longer needed because we're don't have a userspace, and there's no real reason to 
        // place the kernel in the higher half. 
        //
        // These identity mappings are short-lived; they are unmapped later after all other CPUs are brought up
        // but before we start running applications.

        debug!("{:X?}", aggregated_section_memory_bounds);
        let mut index = 0;

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

        let text_pages = page_allocator::allocate_pages_by_bytes_at(text_start_virt, text_end_virt.value() - text_start_virt.value())?;
        let text_frames = frame_allocator::allocate_frames_by_bytes_at(text_start_phys, text_end_phys.value() - text_start_phys.value())?;
        let text_pages_identity = page_allocator::allocate_pages_by_bytes_at(text_start_virt - KERNEL_OFFSET, text_end_virt.value() - text_start_virt.value())?;
        let text_frames_identity = text_frames.deref().clone();
        text_mapped_pages = Some(mapper.map_allocated_pages_to(text_pages, text_frames, text_flags)?);
        identity_mapped_pages[index] = Some( unsafe {
            Mapper::map_to_non_exclusive(mapper, text_pages_identity, text_frames_identity, text_flags)?
        });
        index += 1;

        let rodata_pages = page_allocator::allocate_pages_by_bytes_at(rodata_start_virt, rodata_end_virt.value() - rodata_start_virt.value())?;
        let rodata_frames = frame_allocator::allocate_frames_by_bytes_at(rodata_start_phys, rodata_end_phys.value() - rodata_start_phys.value())?;
        let rodata_pages_identity = page_allocator::allocate_pages_by_bytes_at(rodata_start_virt - KERNEL_OFFSET, rodata_end_virt.value() - rodata_start_virt.value())?;
        let rodata_frames_identity = rodata_frames.deref().clone();
        rodata_mapped_pages = Some(mapper.map_allocated_pages_to(rodata_pages, rodata_frames, rodata_flags)?);
        identity_mapped_pages[index] = Some( unsafe {
            Mapper::map_to_non_exclusive(mapper, rodata_pages_identity, rodata_frames_identity, rodata_flags)?
        });
        index += 1;

        let data_pages = page_allocator::allocate_pages_by_bytes_at(data_start_virt, data_end_virt.value() - data_start_virt.value())?;
        let data_frames = frame_allocator::allocate_frames_by_bytes_at(data_start_phys, data_end_phys.value() - data_start_phys.value())?;
        let data_pages_identity = page_allocator::allocate_pages_by_bytes_at(data_start_virt - KERNEL_OFFSET, data_end_virt.value() - data_start_virt.value())?;
        let data_frames_identity = data_frames.deref().clone();
        data_mapped_pages = Some(mapper.map_allocated_pages_to(data_pages, data_frames, data_flags)?);
        identity_mapped_pages[index] = Some( unsafe {
            Mapper::map_to_non_exclusive(mapper, data_pages_identity, data_frames_identity, data_flags)?
        });
        index += 1;

        // We don't need to do any mapping for the initial root (P4) page table stack (a separate data section),
        // which was initially set up and created by the bootstrap assembly code. 
        // It was used to bootstrap the initial page table at the beginning of this function. 

        // Handle the stack (a separate data section), which consists of one guard page followed by the real stack pages.
        // It does not need to be identity mapped because each AP core will have its own stack.
        let stack_pages = page_allocator::allocate_pages_by_bytes_at(stack_start_virt, (stack_end_virt - stack_start_virt).value())?;
        let start_of_stack_pages = *stack_pages.start() + 1; 
        let (stack_guard_page, stack_allocated_pages) = stack_pages.split(start_of_stack_pages)
            .map_err(|_ap| "BUG: initial stack's allocated pages were not split correctly after guard page")?;
        let stack_start_frame = Frame::containing_address(stack_start_phys) + 1; // skip 1st frame, which corresponds to the guard page
        let stack_allocated_frames = frame_allocator::allocate_frames_at(stack_start_frame.start_address(), stack_allocated_pages.size_in_pages())?;
        let stack_mapped_pages = mapper.map_allocated_pages_to(
            stack_allocated_pages,
            stack_allocated_frames,
            data_flags,
        )?;
        stack_page_group = Some((stack_guard_page, stack_mapped_pages));

        // Map the VGA display memory as writable. 
        // We do an identity mapping for the VGA display too, because the AP cores may access it while booting.
        let (vga_phys_addr, vga_size_in_bytes, vga_flags) = get_vga_mem_addr()?;
        let vga_virt_addr_identity = VirtualAddress::new_canonical(vga_phys_addr.value());
        let vga_display_pages = page_allocator::allocate_pages_by_bytes_at(vga_virt_addr_identity + KERNEL_OFFSET, vga_size_in_bytes)?;
        let vga_display_frames = frame_allocator::allocate_frames_by_bytes_at(vga_phys_addr, vga_size_in_bytes)?;
        let vga_display_pages_identity = page_allocator::allocate_pages_by_bytes_at(vga_virt_addr_identity, vga_size_in_bytes)?;
        let vga_display_frames_identity = vga_display_frames.deref().clone();
        higher_half_mapped_pages[index] = Some(mapper.map_allocated_pages_to(vga_display_pages, vga_display_frames, vga_flags)?);
        identity_mapped_pages[index] = Some( unsafe {
            Mapper::map_to_non_exclusive(mapper, vga_display_pages_identity, vga_display_frames_identity, vga_flags)?
        });
        index += 1;


        // Map the multiboot boot_info at the same address it is currently at, so we can continue to validly access `boot_info`
        let boot_info_pages = page_allocator::allocate_pages_by_bytes_at(boot_info_start_vaddr, boot_info_size)?;
        let boot_info_frames = frame_allocator::allocate_frames_by_bytes_at(boot_info_start_paddr, boot_info_size)?;
        boot_info_mapped_pages = Some(mapper.map_allocated_pages_to(
            boot_info_pages,
            boot_info_frames,
            EntryFlags::PRESENT,
        )?);

        debug!("identity_mapped_pages: {:?}", &identity_mapped_pages[..index]);
        debug!("higher_half_mapped_pages: {:?}", &higher_half_mapped_pages[..index]);

        Ok(()) // mapping closure completed successfully

    })?; // TemporaryPage is dropped here


    let text_mapped_pages       = text_mapped_pages     .ok_or("Couldn't map .text section")?;
    let rodata_mapped_pages     = rodata_mapped_pages   .ok_or("Couldn't map .rodata section")?;
    let data_mapped_pages       = data_mapped_pages     .ok_or("Couldn't map .data section")?;
    let boot_info_mapped_pages  = boot_info_mapped_pages.ok_or("Couldn't map boot_info pages section")?;
    let stack_page_group        = stack_page_group      .ok_or("Couldn't map .stack section")?;

    debug!("switching from old page table {:?} to new page table {:?}", page_table, new_table);
    page_table.switch(&new_table); 
    debug!("switched to new page table {:?}.", new_table); 
    // The old page_table set up during bootstrap will be dropped here. It's no longer being used.

    // Return the new page table because that's the one that should be used by the kernel in future mappings. 
    Ok((
        new_table,
        text_mapped_pages,
        rodata_mapped_pages,
        data_mapped_pages,
        stack_page_group,
        boot_info_mapped_pages,
        higher_half_mapped_pages,
        identity_mapped_pages
    ))
}
