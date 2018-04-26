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
mod table;
mod temporary_page;
mod mapper;


pub use self::entry::*;
pub use self::temporary_page::TemporaryPage;
pub use self::mapper::*;
pub use self::virtual_address_allocator::*;


use core::ops::{Add, AddAssign, Sub, SubAssign, Deref, DerefMut};
use multiboot2;
use super::*;

use x86_64::registers::control_regs;
use x86_64::instructions::tlb;

use kernel_config::memory::{PAGE_SIZE, MAX_PAGE_NUMBER, RECURSIVE_P4_INDEX, address_is_page_aligned};
use kernel_config::memory::{KERNEL_TEXT_P4_INDEX, KERNEL_HEAP_P4_INDEX, KERNEL_STACK_P4_INDEX};




#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Page {
    number: usize, 
}
impl fmt::Debug for Page {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Page(vaddr: {:#X})", self.start_address()) 
    }
}

impl Page {
    /// Returns true if the given `VirtualAddress` is canonical, i.e., within the valid range.
    pub fn is_valid_address(address: VirtualAddress) -> bool {
        address < 0x0000_8000_0000_0000 || address >= 0xffff_8000_0000_0000
    }

	/// returns the first virtual address as the start of this Page
    pub fn containing_address(address: VirtualAddress) -> Page {
        assert!(Page::is_valid_address(address), "Page::containing_address(): invalid address: 0x{:x}", address);
        Page { number: address / PAGE_SIZE }
    }

    pub fn start_address(&self) -> usize {
        self.number * PAGE_SIZE
    }

	/// returns the 9-bit part of this page's virtual address that is the index into the P4 page table entries list
    fn p4_index(&self) -> usize {
        (self.number >> 27) & 0o777
    }

    /// returns the 9-bit part of this page's virtual address that is the index into the P3 page table entries list
    fn p3_index(&self) -> usize {
        (self.number >> 18) & 0o777
    }

    /// returns the 9-bit part of this page's virtual address that is the index into the P2 page table entries list
    fn p2_index(&self) -> usize {
        (self.number >> 9) & 0o777
    }

    /// returns the 9-bit part of this page's virtual address that is the index into the P2 page table entries list.
    /// using this returned usize value as an index into the P1 entries list will give you the final PTE, 
    /// from which you can extract the physical address using pointed_frame()
    fn p1_index(&self) -> usize {
        (self.number >> 0) & 0o777
    }

    pub fn range_inclusive(start: Page, end: Page) -> PageIter {
        PageIter::new(start, end)
    }

    pub fn range_inclusive_addr(virt_addr: VirtualAddress, size_in_bytes: usize) -> PageIter {
        let start_page = Page::containing_address(virt_addr);
        let end_page = Page::containing_address(virt_addr + size_in_bytes - 1);
        PageIter::new(start_page, end_page)
    }
}

impl Add<usize> for Page {
    type Output = Page;

    fn add(self, rhs: usize) -> Page {
        assert!(self.number < MAX_PAGE_NUMBER, "Page addition error, cannot go above MAX_PAGE_NUMBER 0x000FFFFFFFFFFFFF!");
        Page { number: self.number + rhs }
    }
}

impl AddAssign<usize> for Page {
    fn add_assign(&mut self, rhs: usize) {
        *self = Page {
            number: self.number + rhs,
        };
    }
}

impl Sub<usize> for Page {
    type Output = Page;

    fn sub(self, rhs: usize) -> Page {
        assert!(self.number > 0, "Page subtraction error, cannot go below zero!");
        Page { number: self.number - rhs }
    }
}

impl SubAssign<usize> for Page {
    fn sub_assign(&mut self, rhs: usize) {
        *self = Page {
            number: self.number - rhs,
        };
    }
}


/// An iterator over a range of contiguous pages,
/// from `start` to `end`, both inclusive.
#[derive(Debug)]
pub struct PageIter {
    pub start: Page,
    pub end: Page,
    pub current: Page,
}

impl PageIter {
    pub fn new(start: Page, end: Page) -> PageIter {
        PageIter {
            start: start,
            end: end,
            current: start, // start at the beginning
        }
    }

    pub fn empty() -> PageIter {
        PageIter::new(Page { number: 1 }, Page { number: 0 })
    }

    /// Create a duplicate of this PageIter. 
    /// We do this instead of implementing/deriving the Clone trait
    /// because we want to prevent Rust from cloning `PageIter`s implicitly.
    pub fn clone(&self) -> PageIter {
        PageIter {
            start: self.start,
            end: self.end,
            current: self.current,
        }
    }

    pub fn start_address(&self) -> VirtualAddress {
        self.start.start_address()
    }
}

impl Iterator for PageIter {
    type Item = Page;

    fn next(&mut self) -> Option<Page> {
        if self.current <= self.end {
            let page = self.current;
            self.current.number += 1;
            Some(page)
        } else {
            None
        }
    }
}


/// the owner of the recursively defined P4 page table. 
pub struct ActivePageTable {
    mapper: Mapper,
    p4_frame: Frame,
}
impl fmt::Debug for ActivePageTable {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "ActivePageTable(p4: {:#X})", self.p4_frame.start_address()) 
    }
}

impl Deref for ActivePageTable {
    type Target = Mapper;

    fn deref(&self) -> &Mapper {
        &self.mapper
    }
}

impl DerefMut for ActivePageTable {
    fn deref_mut(&mut self) -> &mut Mapper {
        &mut self.mapper
    }
}

impl ActivePageTable {
    fn new(p4: Frame) -> ActivePageTable {
        ActivePageTable { 
            mapper: Mapper::new(),
            p4_frame: p4,
        }
    }

    /// Temporarily maps the given `InactivePageTable` to the recursive entry (510th entry) 
    /// so that we can set up new mappings on the new `table` before actually switching to it.
    /// Accepts a closure that is given a `Mapper` such that it can set up new mappins on the given `InactivePageTable`.
    /// Consumes the given `temporary_page` and automatically unmaps it afterwards. 
    /// Note: THIS DOES NOT PERFORM ANY CONTEXT SWITCHING OR CHANGING OF THE CURRENT PAGE TABLE REGISTER (e.g., CR3)
    pub fn with<F>(&mut self,
                   table: &mut InactivePageTable,
                   mut temporary_page: temporary_page::TemporaryPage,
                   f: F)
        -> Result<(), &'static str>
        where F: FnOnce(&mut Mapper) -> Result<(), &'static str>
    {
        let backup = get_current_p4();

        // map temporary_page to current p4 table
        let p4_table = try!(temporary_page.map_table_frame(backup.clone(), self));

        // overwrite recursive mapping
        self.p4_mut()[RECURSIVE_P4_INDEX].set(table.p4_frame.clone(), EntryFlags::PRESENT | EntryFlags::WRITABLE); 
        tlb::flush_all();

        // set mapper's target frame to reflect that future mappings will be mapped into the new InactivePageTable
        self.mapper.target_p4 = table.p4_frame.clone();

        // execute f in the new context
        let ret = f(self);

        // restore mapper's target frame to reflect that future mappings are mapped using the current ActivePageTable
        self.mapper.target_p4 = self.p4_frame.clone();

        // restore recursive mapping to original p4 table
        p4_table[RECURSIVE_P4_INDEX].set(backup, EntryFlags::PRESENT | EntryFlags::WRITABLE);
        tlb::flush_all();

        // here, temporary_page is dropped, which auto unmaps it
        ret
    }


    /// returns the newly-created ActivePageTable, based on the given new_table.
    /// No need to return the old_table as an InactivePageTable, since we're not doing that anymore.
    /// Instead, the old_table remains "active" because other cores may use it.
    pub fn switch(&mut self, new_table: &PageTable) -> ActivePageTable {
        use x86_64::PhysicalAddress;
        // debug!("ActivePageTable::switch() old table: {:?}, new table: {:?}", self, new_table);

        // if this is the first time the new page table has been used, it will be an InactivePageTable,
        // otherwise, it will already be an ActivePageTable. 
        // Either way, we need its p4_frame value, since that's what we'll be changing cr3 to. 
        let new_active_table_p4: Frame = match new_table {
            &PageTable::Inactive(ref inactive_table) => inactive_table.p4_frame.clone(),
            &PageTable::Active(ref active_table) => active_table.p4_frame.clone(),
        };

        // perform the actual page table switch
        unsafe {
            control_regs::cr3_write(PhysicalAddress(new_active_table_p4.start_address() as u64));
        }
        // debug!("ActivePageTable::switch(): NEW TABLE!!!");
        
        ActivePageTable::new(new_active_table_p4)
    }


    /// Returns the physical address of this page table's top-level p4 frame
    pub fn physical_address(&self) -> PhysicalAddress {
        self.p4_frame.start_address()
    }
}




pub struct InactivePageTable {
    p4_frame: Frame,
}
impl fmt::Debug for InactivePageTable {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "InactivePageTable(p4: {:#X})", self.p4_frame.start_address()) 
    }
}

impl InactivePageTable {
    /// Initializes a new InactivePageTable using the given `TemporaryPage`.
    /// The `TemporaryPage` is consumed here.
    pub fn new(frame: Frame,
               active_table: &mut ActivePageTable,
               mut temporary_page: TemporaryPage)
               -> Result<InactivePageTable, &'static str> {
        {
            let table = try!(temporary_page.map_table_frame(frame.clone(), active_table));
            table.zero();

            table[RECURSIVE_P4_INDEX].set(frame.clone(), EntryFlags::PRESENT | EntryFlags::WRITABLE);

            // start out by copying all the kernel sections into the new inactive table
            table.copy_entry_from_table(active_table.p4(), KERNEL_TEXT_P4_INDEX);
            table.copy_entry_from_table(active_table.p4(), KERNEL_HEAP_P4_INDEX);
            table.copy_entry_from_table(active_table.p4(), KERNEL_STACK_P4_INDEX);
        }

        Ok(
            InactivePageTable { p4_frame: frame }
        )

        // temporary_page is auto unmapped here 
        // temporary_page.unmap(active_table);
    }
}


#[derive(Debug)]
pub enum PageTable {
    Active(ActivePageTable),
    Inactive(InactivePageTable),
}


/// Returns the current top-level page table frame, e.g., cr3 on x86
pub fn get_current_p4() -> Frame {
    Frame::containing_address(control_regs::cr3().0 as usize)
}


#[derive(Copy, Clone, Default)]
pub struct TempModule {
    mod_start_paddr: u32,
    mod_end_paddr: u32,
    name: &'static str,
}
impl fmt::Debug for TempModule {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "TempModule(\"{}\", start: {:#X}, end: {:#X})", 
                   self.name, self.mod_start_paddr, self.mod_end_paddr) 
    }
}


/// Initializes a new page table and sets up all necessary mappings for the kernel to continue running. 
/// Returns the following tuple, if successful:
/// 
///  * The kernel's new ActivePageTable,
///  * the kernel's list of VirtualMemoryAreas,
///  * the kernels' text section MappedPages,
///  * the kernels' rodata section MappedPages,
///  * the kernels' data section MappedPages,
///  * the kernel's list of *other* higher-half MappedPages, which should be kept forever,
///  * the kernel's list of identity-mapped MappedPages, which should be dropped before starting the first userspace program. 
///
/// Otherwise, it returns a str error message. 
/// 
/// Note: this was previously called remap_the_kernel.
pub fn init(allocator_mutex: &MutexIrqSafe<AreaFrameAllocator>, boot_info: &multiboot2::BootInformation) 
    -> Result<(ActivePageTable, Vec<VirtualMemoryArea>, MappedPages, MappedPages, MappedPages, Vec<MappedPages>, Vec<MappedPages>), &'static str>
{
    // bootstrap an active_table from the currently-loaded page table
    let mut active_table: ActivePageTable = ActivePageTable::new(get_current_p4());

    let boot_info_start_vaddr: VirtualAddress  = boot_info.start_address();
    let boot_info_start_paddr: PhysicalAddress = try!(active_table.translate(boot_info_start_vaddr)
                                                     .ok_or("Couldn't get boot_info physical address")
    );
    let boot_info_end_vaddr: VirtualAddress  = boot_info.end_address();
    let boot_info_end_paddr: PhysicalAddress = try!(active_table.translate(boot_info_end_vaddr)
                                                   .ok_or("Couldn't get boot_info physical address")
    );
    let boot_info_size = boot_info.total_size();
    // print_raw!("multiboot start: {:#X}-->{:#X}, multiboot end: {:#X}-->{:#X}, size: {:#X}\n",
    //         boot_info_start_vaddr, boot_info_start_paddr, boot_info_end_vaddr, boot_info_end_paddr, boot_info_size
    // );
    info!("multiboot start: {:#X}-->{:#X}, multiboot end: {:#X}-->{:#X}, size: {:#X}\n",
            boot_info_start_vaddr, boot_info_start_paddr, boot_info_end_vaddr, boot_info_end_paddr, boot_info_size
    );

    // frame is a single frame, and temp_frames1/2 are tuples of 3 Frames each.
    let (frame, temp_frames1, temp_frames2) = {
        let mut allocator = allocator_mutex.lock();
        // a quick closure to allocate one frame
        let mut alloc_frame = || allocator.allocate_frame().ok_or("couldn't allocate frame"); 
        (
            try!(alloc_frame()),
            (try!(alloc_frame()), try!(alloc_frame()), try!(alloc_frame())),
            (try!(alloc_frame()), try!(alloc_frame()), try!(alloc_frame()))
        )
    };
    let mut new_table: InactivePageTable = {
        try!(InactivePageTable::new(frame, &mut active_table, TemporaryPage::new(temp_frames1)))
    };

    let elf_sections_tag = try!(boot_info.elf_sections_tag().ok_or("no Elf sections tag present!"));   
    let mut vmas: [VirtualMemoryArea; 32] = Default::default();
    let mut text_mapped_pages: Option<MappedPages> = None;
    let mut rodata_mapped_pages: Option<MappedPages> = None;
    let mut data_mapped_pages: Option<MappedPages> = None;
    let mut higher_half_mapped_pages: [Option<MappedPages>; 32] = Default::default();
    let mut identity_mapped_pages: [Option<MappedPages>; 32] = Default::default();

    // consumes and auto unmaps temporary page
    try!( active_table.with(&mut new_table, TemporaryPage::new(temp_frames2), |mapper| {
        
        // clear out the initially-mapped kernel entries of P4, since we're recreating kernel page tables from scratch.
        // (they were initialized in InactivePageTable::new())
        mapper.p4_mut().clear_entry(KERNEL_TEXT_P4_INDEX);
        mapper.p4_mut().clear_entry(KERNEL_HEAP_P4_INDEX);
        mapper.p4_mut().clear_entry(KERNEL_STACK_P4_INDEX);


        let mut text_start:   Option<(VirtualAddress, PhysicalAddress)> = None;
        let mut text_end:     Option<(VirtualAddress, PhysicalAddress)> = None;
        let mut rodata_start: Option<(VirtualAddress, PhysicalAddress)> = None;
        let mut rodata_end:   Option<(VirtualAddress, PhysicalAddress)> = None;
        let mut data_start:   Option<(VirtualAddress, PhysicalAddress)> = None;
        let mut data_end:     Option<(VirtualAddress, PhysicalAddress)> = None;

        let mut text_flags:       Option<EntryFlags> = None;
        let mut rodata_flags:     Option<EntryFlags> = None;
        let mut data_flags:       Option<EntryFlags> = None;


        // scoped to release the frame allocator lock
        {
            let mut allocator = allocator_mutex.lock(); 

            let mut index = 0;    
            // map the allocated kernel text sections
            for section in elf_sections_tag.sections() {
                
                // skip sections that don't need to be loaded into memory
                if section.size() == 0 
                    || !section.is_allocated() 
                    || section.name().starts_with(".gcc")
                    || section.name().starts_with(".eh_frame")
                    || section.name().starts_with(".debug") 
                {
                    continue;
                }
                
                debug!("Looking at loaded section {} at {:#X}, size {:#X}", section.name(), section.start_address(), section.size());

                if !address_is_page_aligned(section.start_address() as usize) {
                    error!("Section {} at {:#X}, size {:#X} was not page-aligned!", section.name(), section.start_address(), section.size());
                    return Err("Kernel ELF Section was not page-aligned");
                }

                let flags = EntryFlags::from_multiboot2_section_flags(&section) | EntryFlags::GLOBAL;

                // even though the linker stipulates that the kernel sections have a higher-half virtual address,
                // they are still loaded at a lower physical address, in which phys_addr = virt_addr - KERNEL_OFFSET.
                // thus, we must map the zeroeth kernel section from its low address to a higher-half address,
                // and we must map all the other sections from their higher given virtual address to the proper lower phys addr
                let mut start_phys_addr = section.start_address() as PhysicalAddress;
                if start_phys_addr >= KERNEL_OFFSET { 
                    // true for all sections but the first section (inittext)
                    start_phys_addr -= KERNEL_OFFSET;
                }
                
                let mut start_virt_addr = section.start_address() as VirtualAddress;
                if start_virt_addr < KERNEL_OFFSET { 
                    // special case to handle the first section only
                    start_virt_addr += KERNEL_OFFSET;
                }

                let end_virt_addr = start_virt_addr + section.size() as VirtualAddress;
                let end_phys_addr = start_phys_addr + section.size() as PhysicalAddress;


                // the linker script (linker_higher_half.ld) defines the following order of sections:
                //     .init (start) then .text (end)
                //     .data (start) then .bss (end)
                //     .rodata (start and end)
                // Those are the only sections we care about.
                let static_str_name = match section.name() {
                    ".init" => {
                        text_start = Some((start_virt_addr, start_phys_addr));
                        "nano_core .init"
                    } 
                    ".text" => {
                        text_end = Some((end_virt_addr, end_phys_addr));
                        text_flags = Some(flags);
                        "nano_core .text"
                    }
                    ".rodata" => {
                        rodata_start = Some((start_virt_addr, start_phys_addr));
                        rodata_end   = Some((end_virt_addr, end_phys_addr));
                        rodata_flags = Some(flags);
                        "nano_core .rodata"
                    }
                    ".data" => {
                        data_start = Some((start_virt_addr, start_phys_addr));
                        data_flags = Some(flags);
                        "nano_core .data"
                    }
                    ".bss" => {
                        data_end = Some((end_virt_addr, end_phys_addr));
                        "nano_core .bss"
                    }
                    _ =>  {
                        error!("Section {} at {:#X}, size {:#X} was not an expected section (.init, .text, .data, .bss, .rodata)", 
                                section.name(), section.start_address(), section.size());
                        return Err("Kernel ELF Section had an unexpected name (expected .init, .text, .data, .bss, .rodata)");
                    }
                };
                vmas[index] = VirtualMemoryArea::new(start_virt_addr, section.size() as usize, flags, static_str_name);
                debug!("     mapping kernel section: {} at addr: {:?}", section.name(), vmas[index]);


                // to allow the APs to boot up, we identity map the kernel sections too.
                // (lower half virtual addresses mapped to same lower half physical addresses)
                // we will unmap these later before we start booting to userspace processes
                identity_mapped_pages[index] = Some(try!( mapper.map_frames(
                    Frame::range_inclusive_addr(start_phys_addr, section.size() as usize), 
                    Page::containing_address(start_virt_addr - KERNEL_OFFSET), 
                    flags, allocator.deref_mut())
                ));
                debug!("           also mapped vaddr {:#X} to paddr {:#x} (size {:#X})", start_virt_addr - KERNEL_OFFSET, start_phys_addr, section.size());

                index += 1;      

            } // end of section iterator


            let (text_start_virt,    text_start_phys)    = try!(text_start  .ok_or("Couldn't find start of .text section"));
            let (_text_end_virt,     text_end_phys)      = try!(text_end    .ok_or("Couldn't find end of .text section"));
            let (rodata_start_virt,  rodata_start_phys)  = try!(rodata_start.ok_or("Couldn't find start of .rodata section"));
            let (_rodata_end_virt,   rodata_end_phys)    = try!(rodata_end  .ok_or("Couldn't find end of .rodata section"));
            let (data_start_virt,    data_start_phys)    = try!(data_start  .ok_or("Couldn't find start of .data section"));
            let (_data_end_virt,     data_end_phys)      = try!(data_end    .ok_or("Couldn't find start of .data section"));

            let text_flags    = try!(text_flags  .ok_or("Couldn't find .text section flags"));
            let rodata_flags  = try!(rodata_flags.ok_or("Couldn't find .rodata section flags"));
            let data_flags    = try!(data_flags  .ok_or("Couldn't find .data section flags"));


            // now we map the 5 main sections into 3 groups according to flags
            text_mapped_pages = Some( try!( mapper.map_frames(
                Frame::range_inclusive_addr(text_start_phys, text_end_phys - text_start_phys), 
                Page::containing_address(text_start_virt), 
                text_flags, allocator.deref_mut())
            ));
            rodata_mapped_pages = Some( try!( mapper.map_frames(
                Frame::range_inclusive_addr(rodata_start_phys, rodata_end_phys - rodata_start_phys), 
                Page::containing_address(rodata_start_virt), 
                rodata_flags, allocator.deref_mut())
            ));
            data_mapped_pages = Some( try!( mapper.map_frames(
                Frame::range_inclusive_addr(data_start_phys, data_end_phys - data_start_phys),
                Page::containing_address(data_start_virt), 
                data_flags, allocator.deref_mut())
            ));


            const VGA_DISPLAY_PHYS_START: PhysicalAddress = 0xB_8000;
            const VGA_DISPLAY_PHYS_END: PhysicalAddress = 0xC_0000;

            // map the VGA display memory as writable, which technically goes from 0xA_0000 - 0xC_0000 (exclusive),
            // but currently we're only using VGA text mode, which goes from 0xB_8000 - 0XC_0000
            let vga_display_virt_addr: VirtualAddress = VGA_DISPLAY_PHYS_START + KERNEL_OFFSET;
            let size_in_bytes: usize = VGA_DISPLAY_PHYS_END - VGA_DISPLAY_PHYS_START;
            let vga_display_flags = EntryFlags::PRESENT | EntryFlags::WRITABLE | EntryFlags::GLOBAL | EntryFlags::NO_CACHE;
            higher_half_mapped_pages[index] = Some( try!( mapper.map_frames(
                Frame::range_inclusive_addr(VGA_DISPLAY_PHYS_START, size_in_bytes), 
                Page::containing_address(vga_display_virt_addr), 
                vga_display_flags, allocator.deref_mut())
            ));
            vmas[index] = VirtualMemoryArea::new(vga_display_virt_addr, size_in_bytes, vga_display_flags, "Kernel VGA Display Memory");
            debug!("mapped kernel section: vga_buffer at addr: {:?}", vmas[index]);
            // also do an identity mapping for APs that need it while booting
            identity_mapped_pages[index] = Some( try!( mapper.map_frames(
                Frame::range_inclusive_addr(VGA_DISPLAY_PHYS_START, size_in_bytes), 
                Page::containing_address(vga_display_virt_addr - KERNEL_OFFSET), 
                vga_display_flags, allocator.deref_mut())
            ));
            index += 1;
            

            // map the multiboot boot_info at the same address it previously was, so we can continue to access boot_info 
            let boot_info_pages  = Page::range_inclusive_addr(boot_info_start_vaddr, boot_info_size);
            let boot_info_frames = Frame::range_inclusive_addr(boot_info_start_paddr, boot_info_size);
            vmas[index] = VirtualMemoryArea::new(boot_info_start_vaddr, boot_info_size, EntryFlags::PRESENT | EntryFlags::GLOBAL, "Kernel Multiboot Info");
            for (page, frame) in boot_info_pages.zip(boot_info_frames) {
                // we must do it page-by-page to make sure that a page hasn't already been mapped
                if mapper.translate_page(page).is_some() {
                    // skip pages that are already mapped
                    continue;
                }
                // print_raw!("MAPPING BOOT_INFO PAGE {:?}\n", page);
                higher_half_mapped_pages[index] = Some( try!( mapper.map_to(
                    page, frame.clone(), EntryFlags::PRESENT | EntryFlags::GLOBAL, allocator.deref_mut())
                ));
                // print_raw!("mapped bootloader info at addr: {:?}\n", vmas[index]);
                // also do an identity mapping, if maybe we need it?
                identity_mapped_pages[index] = Some( try!( mapper.map_to(
                    Page::containing_address(page.start_address() - KERNEL_OFFSET), frame, 
                    EntryFlags::PRESENT | EntryFlags::GLOBAL, allocator.deref_mut())
                ));
                index += 1;
            }

            debug!("identity_mapped_pages: {:?}", &identity_mapped_pages[0..(index + 1)]);

        } // unlocks the frame allocator 

        Ok(()) // mapping closure completed successfully

    })); // TemporaryPage is dropped here


    let text_mapped_pages   = try!(text_mapped_pages  .ok_or("Couldn't map .text section"));
    let rodata_mapped_pages = try!(rodata_mapped_pages.ok_or("Couldn't map .rodata section"));
    let data_mapped_pages   = try!(data_mapped_pages  .ok_or("Couldn't map .data section"));


    debug!("switching to new page table {:?}", new_table);
    let mut new_active_table = active_table.switch(&PageTable::Inactive(new_table));
    debug!("switched to new page table {:?}.", new_active_table);           


    // We must map the heap memory here, before it can initialized! 
    let (heap_mapped_pages, heap_vma) = {
        let mut allocator = allocator_mutex.lock();

        use heap_irq_safe;
        let pages = Page::range_inclusive_addr(KERNEL_HEAP_START, KERNEL_HEAP_INITIAL_SIZE);
        let heap_flags = paging::EntryFlags::WRITABLE;
        let heap_vma: VirtualMemoryArea = VirtualMemoryArea::new(KERNEL_HEAP_START, KERNEL_HEAP_INITIAL_SIZE, heap_flags, "Kernel Heap");
        let heap_mp = try!(new_active_table.map_pages(pages, heap_flags, allocator.deref_mut()));
        heap_irq_safe::init(KERNEL_HEAP_START, KERNEL_HEAP_INITIAL_SIZE);
        
        allocator.alloc_ready(); // heap is ready
        (heap_mp, heap_vma)
    };

    // debug!("mapped and inited the heap, VMA: {:?}", heap_vma);
    // HERE: now the heap is set up, we can use dynamically-allocated types like Vecs

    let mut kernel_vmas: Vec<VirtualMemoryArea> = vmas.to_vec();
    kernel_vmas.retain(|x|  *x != VirtualMemoryArea::default() );
    kernel_vmas.push(heap_vma);

    debug!("kernel_vmas: {:?}", kernel_vmas);

    let mut higher_half: Vec<MappedPages> = higher_half_mapped_pages.iter_mut().filter_map(|opt| opt.take()).collect();
    higher_half.push(heap_mapped_pages);
    let identity: Vec<MappedPages> = identity_mapped_pages.iter_mut().filter_map(|opt| opt.take()).collect();

    // Return the new_active_table because that's the one that should be used by the kernel (task_zero) in future mappings. 
    Ok((new_active_table, kernel_vmas, text_mapped_pages, rodata_mapped_pages, data_mapped_pages, higher_half, identity))
}


/// Get a stack trace, borrowed from Redox
/// TODO: Check for stack being mapped before dereferencing
#[inline(never)]
pub fn stack_trace() {
    use core::mem;

    // SAFE, just a stack trace for debugging purposes, and pointers are checked. 
    unsafe {
        
        // get the stack base pointer
        let mut rbp: usize;
        asm!("" : "={rbp}"(rbp) : : : "intel", "volatile");

        // println_raw!("TRACE: {:>016X}", rbp);
        error!("STACK TRACE: {:>016X}", rbp);
        //Maximum 64 frames
        let active_table = ActivePageTable::new(get_current_p4());
        for _frame in 0..64 {
            if let Some(rip_rbp) = rbp.checked_add(mem::size_of::<usize>()) {
                if Page::is_valid_address(rbp) && Page::is_valid_address(rip_rbp) {
                    // println_raw!(" {:>016X}: INVALID ADDRESS", rbp);
                    error!(" {:>016X}: INVALID_ADDRESS", rbp);
                    break;
                }
                
                if active_table.translate(rbp).is_some() && active_table.translate(rip_rbp).is_some() {
                    let rip = *(rip_rbp as *const usize);
                    if rip == 0 {
                        // println_raw!(" {:>016X}: EMPTY RETURN", rbp);
                        error!(" {:>016X}: EMPTY RETURN", rbp);
                        break;
                    }
                    // println_raw!("  {:>016X}: {:>016X}", rbp, rip);
                    error!("  {:>016X}: {:>016X}", rbp, rip);
                    rbp = *(rbp as *const usize);
                    // symbol_trace(rip);
                } else {
                    // println_raw!("  {:>016X}: GUARD PAGE", rbp);
                    error!("  {:>016X}: GUARD PAGE", rbp);
                    break;
                }
            } else {
                // println_raw!("  {:>016X}: RBP OVERFLOW", rbp);
                error!("  {:>016X}: RBP OVERFLOW", rbp);
            }
        }
    }
}

