// Copyright 2016 Philipp Oppermann. See the README.md
// file at the top-level directory of this distribution.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

mod virtual_address_allocator;
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
pub use self::virtual_address_allocator::*;

use core::{
    ops::{Deref, DerefMut},
    fmt,
};
use super::*;

use kernel_config::memory::{RECURSIVE_P4_INDEX};
use kernel_config::memory::{KERNEL_TEXT_P4_INDEX, KERNEL_HEAP_P4_INDEX, KERNEL_STACK_P4_INDEX};


/// A root (P4) page table.
/// 
/// Auto-derefs into a `Mapper` for easy invocation of memory mapping functions.
pub struct PageTable {
    mapper: Mapper,
    p4_table: Frame,
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
    fn from_current() -> PageTable {
        PageTable { 
            mapper: Mapper::from_current(),
            p4_table: get_current_p4(),
        }
    }

    /// Initializes a brand new top-level P4 `PageTable` (previously called an `InactivePageTable`)
    /// that is based on the given `current_active_table` and is located in the given `new_p4_frame`.
    /// The `TemporaryPage` is used for recursive mapping, and is auto-unmapped upon return. 
    /// 
    /// Returns the new `PageTable` that exists in physical memory at the given `new_p4_frame`, 
    /// and has the kernel memory region mappings copied in from the given `current_page_table`
    /// to ensure that the system will continue running 
    pub fn new_table(
        current_page_table: &mut PageTable,
        new_p4_frame: Frame,
        mut temporary_page: TemporaryPage,
    ) -> Result<PageTable, &'static str> {
        {
            let table = temporary_page.map_table_frame(new_p4_frame.clone(), current_page_table)?;
            table.zero();

            table[RECURSIVE_P4_INDEX].set(new_p4_frame.clone(), EntryFlags::PRESENT | EntryFlags::WRITABLE);

            // start out by copying all the kernel sections into the new table
            table.copy_entry_from_table(current_page_table.p4(), KERNEL_TEXT_P4_INDEX);
            table.copy_entry_from_table(current_page_table.p4(), KERNEL_HEAP_P4_INDEX);
            table.copy_entry_from_table(current_page_table.p4(), KERNEL_STACK_P4_INDEX);
            // TODO: FIXME: we should probably copy all of the mappings here just to be safe (except 510, the recursive P4 entry.)
        }

        Ok( PageTable { 
            mapper: Mapper::with_p4_frame(new_p4_frame.clone()),
            p4_table: new_p4_frame 
        })
        // temporary_page is auto unmapped here 
    }

    /// Temporarily maps the given other `PageTable` to the recursive entry (510th entry) 
    /// so that the given closure `f` can set up new mappings on the new `other_table` without actually switching to it yet.
    /// Accepts a closure `f` that is passed  a `Mapper`, such that it can set up new mappings on the other table.
    /// Consumes the given `temporary_page` and automatically unmaps it afterwards. 
    /// # Note
    /// This does not perform any task switching or changing of the current page table register (e.g., cr3).
    pub fn with<F>(&mut self,
                   other_table: &mut PageTable,
                   mut temporary_page: temporary_page::TemporaryPage,
                   f: F)
        -> Result<(), &'static str>
        where F: FnOnce(&mut Mapper) -> Result<(), &'static str>
    {
        let backup = get_current_p4();
        if self.p4_table != backup {
            return Err("To invoke PageTable::with(), that PageTable ('self') must be currently active.");
        }

        // map temporary_page to current p4 table
        let p4_table = temporary_page.map_table_frame(backup.clone(), self)?;

        // overwrite recursive mapping
        self.p4_mut()[RECURSIVE_P4_INDEX].set(other_table.p4_table.clone(), EntryFlags::PRESENT | EntryFlags::WRITABLE); 
        tlb_flush_all();

        // set mapper's target frame to reflect that future mappings will be mapped into the other_table
        self.mapper.target_p4 = other_table.p4_table.clone();

        // execute f in the new context
        let ret = f(self);

        // restore mapper's target frame to reflect that future mappings will be mapped using the currently-active (original) PageTable
        self.mapper.target_p4 = self.p4_table.clone();

        // restore recursive mapping to original p4 table
        p4_table[RECURSIVE_P4_INDEX].set(backup, EntryFlags::PRESENT | EntryFlags::WRITABLE);
        tlb_flush_all();

        // here, temporary_page is dropped, which auto unmaps it
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
///  * the kernel's list of *other* higher-half MappedPages, which should be kept forever,
///  * the kernel's list of identity-mapped MappedPages, which should be dropped before starting the first userspace program. 
///
/// Otherwise, it returns a str error message. 
pub fn init(allocator_mutex: &MutexIrqSafe<AreaFrameAllocator>, boot_info: &multiboot2::BootInformation) 
    -> Result<(PageTable, MappedPages, MappedPages, MappedPages, Vec<MappedPages>, Vec<MappedPages>), &'static str>
{
    // bootstrap a PageTable from the currently-loaded page table
    let mut page_table = PageTable::from_current();

    let boot_info_start_vaddr = VirtualAddress::new(boot_info.start_address()).map_err(|_| "boot_info start virtual address was invalid")?;
    let boot_info_start_paddr = page_table.translate(boot_info_start_vaddr).ok_or("Couldn't get boot_info start physical address")?;
    let boot_info_size = boot_info.total_size();
    info!("multiboot vaddr: {:#X}, multiboot paddr: {:#X}, size: {:#X}\n", boot_info_start_vaddr, boot_info_start_paddr, boot_info_size);

    // new_frame is a single frame, and temp_frames1/2 are tuples of 3 Frames each.
    let (new_frame, temp_frames1, temp_frames2) = {
        let mut allocator = allocator_mutex.lock();
        // a quick closure to allocate one frame
        let mut alloc_frame = || allocator.allocate_frame().ok_or("couldn't allocate frame"); 
        (
            alloc_frame()?,
            (alloc_frame()?, alloc_frame()?, alloc_frame()?),
            (alloc_frame()?, alloc_frame()?, alloc_frame()?)
        )
    };
    let mut new_table = PageTable::new_table(&mut page_table, new_frame, TemporaryPage::new(temp_frames1))?;

    let mut text_mapped_pages: Option<MappedPages> = None;
    let mut rodata_mapped_pages: Option<MappedPages> = None;
    let mut data_mapped_pages: Option<MappedPages> = None;
    let mut higher_half_mapped_pages: [Option<MappedPages>; 32] = Default::default();
    let mut identity_mapped_pages: [Option<MappedPages>; 32] = Default::default();

    // consumes and auto unmaps temporary page
    page_table.with(&mut new_table, TemporaryPage::new(temp_frames2), |mapper| {
        
        // clear out the initially-mapped kernel entries of P4, since we're recreating kernel page tables from scratch.
        // (they were initialized in PageTable::new_table())
        mapper.p4_mut().clear_entry(KERNEL_TEXT_P4_INDEX);
        mapper.p4_mut().clear_entry(KERNEL_HEAP_P4_INDEX);
        mapper.p4_mut().clear_entry(KERNEL_STACK_P4_INDEX);

        // scoped to release the frame allocator lock
        {
            let mut allocator = allocator_mutex.lock(); 

            let (aggregated_section_memory_bounds, sections_memory_bounds) = find_section_memory_bounds(&boot_info)?;
            
            // Map every section found in the kernel image (given by boot information above) into memory. 
            // To allow the APs to boot up, we identity map those kernel sections too
            // (lower half virtual addresses mapped to same lower half physical addresses).1
            // We will unmap these lower-half identity mappings later, before we start running applications.
            let mut index = 0;
            for sec in sections_memory_bounds.iter().filter_map(|s| s.as_ref()).fuse() {
                let (start_virt_addr, start_phys_addr) = sec.start;
                let (_end_virt_addr, end_phys_addr) = sec.end;
                let size = end_phys_addr.value() - start_phys_addr.value();
                identity_mapped_pages[index] = Some(
                    mapper.map_frames(
                        FrameRange::from_phys_addr(start_phys_addr, size), 
                        Page::containing_address(start_virt_addr - KERNEL_OFFSET), 
                        sec.flags,
                        allocator.deref_mut()
                    )?
                );
                debug!("           also mapped vaddr {:#X} to paddr {:#x} (size {:#X})", start_virt_addr - KERNEL_OFFSET, start_phys_addr, size);
                index += 1;
            }


            let (text_start_virt,    text_start_phys)    = aggregated_section_memory_bounds.text.start;
            let (_text_end_virt,     text_end_phys)      = aggregated_section_memory_bounds.text.end;
            let (rodata_start_virt,  rodata_start_phys)  = aggregated_section_memory_bounds.rodata.start;
            let (_rodata_end_virt,   rodata_end_phys)    = aggregated_section_memory_bounds.rodata.end;
            let (data_start_virt,    data_start_phys)    = aggregated_section_memory_bounds.data.start;
            let (_data_end_virt,     data_end_phys)      = aggregated_section_memory_bounds.data.end;

            let text_flags    = aggregated_section_memory_bounds.text.flags;
            let rodata_flags  = aggregated_section_memory_bounds.rodata.flags;
            let data_flags    = aggregated_section_memory_bounds.data.flags;


            // Now we map all kernel sections into 3 groups according to flags
            text_mapped_pages = Some(mapper.map_frames(
                FrameRange::from_phys_addr(text_start_phys, text_end_phys.value() - text_start_phys.value()), 
                Page::containing_address(text_start_virt), 
                text_flags, allocator.deref_mut()
            )?);
            rodata_mapped_pages = Some(mapper.map_frames(
                FrameRange::from_phys_addr(rodata_start_phys, rodata_end_phys.value() - rodata_start_phys.value()), 
                Page::containing_address(rodata_start_virt), 
                rodata_flags, allocator.deref_mut()
            )?);
            data_mapped_pages = Some(mapper.map_frames(
                FrameRange::from_phys_addr(data_start_phys, data_end_phys.value() - data_start_phys.value()),
                Page::containing_address(data_start_virt), 
                data_flags, allocator.deref_mut()
            )?);

            // map the VGA display memory as writable
            let (vga_display_phys_addr, vga_size_in_bytes, vga_display_flags) = get_vga_mem_addr()?;
            let vga_display_virt_addr = VirtualAddress::new_canonical(vga_display_phys_addr.value() + KERNEL_OFFSET);
            higher_half_mapped_pages[index] = Some(mapper.map_frames(
                FrameRange::from_phys_addr(vga_display_phys_addr, vga_size_in_bytes), 
                Page::containing_address(vga_display_virt_addr), 
                vga_display_flags,
                allocator.deref_mut()
            )?);
            debug!("mapped kernel section: vga_buffer at addr: {:#X}, size {} bytes", vga_display_virt_addr, vga_size_in_bytes);
            // also do an identity mapping for APs that need it while booting
            identity_mapped_pages[index] = Some(mapper.map_frames(
                FrameRange::from_phys_addr(vga_display_phys_addr, vga_size_in_bytes), 
                Page::containing_address(VirtualAddress::new_canonical(vga_display_phys_addr.value())), 
                vga_display_flags, allocator.deref_mut()
            )?);
            index += 1;
            

            // map the multiboot boot_info at the same address it previously was, so we can continue to access boot_info 
            let boot_info_pages  = PageRange::from_virt_addr(boot_info_start_vaddr, boot_info_size);
            let boot_info_frames = FrameRange::from_phys_addr(boot_info_start_paddr, boot_info_size);
            for (page, frame) in boot_info_pages.into_iter().zip(boot_info_frames) {
                // we must do it page-by-page to make sure that a page hasn't already been mapped
                if mapper.translate_page(page).is_some() {
                    // skip pages that are already mapped
                    continue;
                }
                higher_half_mapped_pages[index] = Some(mapper.map_to(
                    page, frame.clone(), EntryFlags::PRESENT | EntryFlags::GLOBAL, allocator.deref_mut()
                )?);
                // also do an identity mapping, if maybe we need it?
                identity_mapped_pages[index] = Some(mapper.map_to(
                    Page::containing_address(page.start_address() - KERNEL_OFFSET), frame, 
                    EntryFlags::PRESENT | EntryFlags::GLOBAL, allocator.deref_mut()
                )?);
                index += 1;
            }

            debug!("identity_mapped_pages: {:?}", &identity_mapped_pages[0..=index]);

        } // unlocks the frame allocator 

        Ok(()) // mapping closure completed successfully

    })?; // TemporaryPage is dropped here


    let text_mapped_pages   = text_mapped_pages  .ok_or("Couldn't map .text section")?;
    let rodata_mapped_pages = rodata_mapped_pages.ok_or("Couldn't map .rodata section")?;
    let data_mapped_pages   = data_mapped_pages  .ok_or("Couldn't map .data section")?;


    debug!("switching to new page table {:?}", new_table);
    let mut new_page_table = page_table.switch(&new_table); 
    // here, new_page_table and new_table should be identical
    debug!("switched to new page table {:?}.", new_page_table); 

    // After this point, we must "forget" all of the above mapped_pages instances if an error occurs,
    // because they will be auto-unmapped from the new page table upon return, causing all execution to stop.          


    // We must map the heap memory here, before it can initialized! 
    let heap_mapped_pages = {
        let mut allocator = allocator_mutex.lock();

        let pages = PageRange::from_virt_addr(VirtualAddress::new_canonical(KERNEL_HEAP_START), KERNEL_HEAP_INITIAL_SIZE);
        let heap_flags = paging::EntryFlags::WRITABLE;
        let heap_mp = try_forget!(
            new_page_table.map_pages(pages, heap_flags, allocator.deref_mut())
                .map_err(|e| {
                    error!("Failed to map kernel heap memory pages, {} bytes starting at virtual address {:#X}. Error: {:?}", KERNEL_HEAP_INITIAL_SIZE, KERNEL_HEAP_START, e);
                    "Failed to map the kernel heap memory. Perhaps the KERNEL_HEAP_INITIAL_SIZE exceeds the size of the system's physical memory?"
                }),
            text_mapped_pages, rodata_mapped_pages, data_mapped_pages, higher_half_mapped_pages, identity_mapped_pages
        );
        heap_irq_safe::init(KERNEL_HEAP_START, KERNEL_HEAP_INITIAL_SIZE);
        
        allocator.alloc_ready(); // heap is ready
        heap_mp
    };

    debug!("mapped and initialized the heap at addr: {:#X}, size {} bytes", heap_mapped_pages.start_address(), heap_mapped_pages.size_in_bytes());
    // HERE: now the heap is set up, we can use dynamically-allocated types like Vecs

    let mut higher_half: Vec<MappedPages> = higher_half_mapped_pages.iter_mut().filter_map(|opt| opt.take()).collect();
    higher_half.push(heap_mapped_pages);
    let identity: Vec<MappedPages> = identity_mapped_pages.iter_mut().filter_map(|opt| opt.take()).collect();

    // Return the new_page_table because that's the one that should be used by the kernel in future mappings. 
    Ok((new_page_table, text_mapped_pages, rodata_mapped_pages, data_mapped_pages, higher_half, identity))
}
