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

#[cfg(mapper_spillful)]
pub mod mapper_spillful;


pub use self::entry::*;
pub use self::temporary_page::TemporaryPage;
pub use self::mapper::*;
pub use self::virtual_address_allocator::*;


use core::{
    ops::{RangeInclusive, Add, AddAssign, Sub, SubAssign, Deref, DerefMut},
    mem,
    iter::Step,
};
use multiboot2;
use super::*;

#[cfg(any(target_arch="x86", target_arch="x86_64"))]
use x86_64::registers::control_regs;
#[cfg(any(target_arch="aarch64"))]
use aarch64::registers::control_regs;
#[cfg(any(target_arch="x86", target_arch="x86_64"))]
use x86_64::instructions::tlb;
#[cfg(any(target_arch="aarch64"))]
use aarch64::instructions::tlb;

use kernel_config::memory::{PAGE_SIZE, MAX_PAGE_NUMBER, RECURSIVE_P4_INDEX};
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
    /// Returns the `Page` that contains the given `VirtualAddress`.
    pub fn containing_address(virt_addr: VirtualAddress) -> Page {
        Page { number: virt_addr.value() / PAGE_SIZE }
    }

	/// Returns the `VirtualAddress` as the start of this `Page`.
    pub fn start_address(&self) -> VirtualAddress {
        VirtualAddress(self.number * PAGE_SIZE)
    }

	/// Returns the 9-bit part of this page's virtual address that is the index into the P4 page table entries list.
    fn p4_index(&self) -> usize {
        (self.number >> 27) & 0x1FF
    }

    /// Returns the 9-bit part of this page's virtual address that is the index into the P3 page table entries list.
    fn p3_index(&self) -> usize {
        (self.number >> 18) & 0x1FF
    }

    /// Returns the 9-bit part of this page's virtual address that is the index into the P2 page table entries list.
    fn p2_index(&self) -> usize {
        (self.number >> 9) & 0x1FF
    }

    /// Returns the 9-bit part of this page's virtual address that is the index into the P2 page table entries list.
    /// Using this returned `usize` value as an index into the P1 entries list will give you the final PTE, 
    /// from which you can extract the mapped `Frame` (or its physical address) using `pointed_frame()`.
    fn p1_index(&self) -> usize {
        (self.number >> 0) & 0x1FF
    }
}

impl Add<usize> for Page {
    type Output = Page;

    fn add(self, rhs: usize) -> Page {
        // cannot exceed max page number
        Page {
            number: core::cmp::min(MAX_PAGE_NUMBER, self.number.saturating_add(rhs)),
        }
    }
}

impl AddAssign<usize> for Page {
    fn add_assign(&mut self, rhs: usize) {
        *self = Page {
            number: core::cmp::min(MAX_PAGE_NUMBER, self.number.saturating_add(rhs)),
        };
    }
}

impl Sub<usize> for Page {
    type Output = Page;

    fn sub(self, rhs: usize) -> Page {
        Page { number: self.number.saturating_sub(rhs) }
    }
}

impl SubAssign<usize> for Page {
    fn sub_assign(&mut self, rhs: usize) {
        *self = Page {
            number: self.number.saturating_sub(rhs),
        };
    }
}

// Implementing these functions allow `Page` to be in an `Iterator`.
impl Step for Page {
    #[inline]
    fn steps_between(start: &Page, end: &Page) -> Option<usize> {
        Step::steps_between(&start.number, &end.number)
    }
    #[inline]
    fn replace_one(&mut self) -> Self {
        mem::replace(self, Page { number: 1 })
    }
    #[inline]
    fn replace_zero(&mut self) -> Self {
        mem::replace(self, Page { number: 0 })
    }
    #[inline]
    fn add_one(&self) -> Self {
        Add::add(*self, 1)
    }
    #[inline]
    fn sub_one(&self) -> Self {
        Sub::sub(*self, 1)
    }
    #[inline]
    fn add_usize(&self, n: usize) -> Option<Page> {
        Some(*self + n)
    }
}



/// A range of `Page`s that are contiguous in virtual memory.
#[derive(Debug, Clone)]
pub struct PageRange(RangeInclusive<Page>);

impl PageRange {
    /// Creates a new range of `Page`s that spans from `start` to `end`,
    /// both inclusive bounds.
    pub fn new(start: Page, end: Page) -> PageRange {
        PageRange(RangeInclusive::new(start, end))
    }

    /// Creates a PageRange that will always yield `None`.
    pub fn empty() -> PageRange {
        PageRange::new(Page { number: 1 }, Page { number: 0 })
    }
    
    /// A convenience method for creating a new `PageRange` 
    /// that spans all `Page`s from the given virtual address 
    /// to an end bound based on the given size.
    pub fn from_virt_addr(starting_virt_addr: VirtualAddress, size_in_bytes: usize) -> PageRange {
        let start_page = Page::containing_address(starting_virt_addr);
        let end_page = Page::containing_address(starting_virt_addr + size_in_bytes - 1);
        PageRange::new(start_page, end_page)
    }

    /// Returns the `VirtualAddress` of the starting `Page` in this `PageRange`.
    pub fn start_address(&self) -> VirtualAddress {
        self.0.start().start_address()
    }

    /// Returns the number of `Page`s covered by this iterator. 
    /// Use this instead of the Iterator trait's `count()` method.
    /// This is instant, because it doesn't need to iterate over each entry, unlike normal iterators.
    pub fn size_in_pages(&self) -> usize {
        // add 1 because it's an inclusive range
        self.0.end().number + 1 - self.0.start().number
    }

    /// Whether this `PageRange` contains the given `VirtualAddress`.
    pub fn contains_virt_addr(&self, virt_addr: VirtualAddress) -> bool {
        self.0.contains(&Page::containing_address(virt_addr))
    }

    /// Returns the offset of the given `VirtualAddress` within this `PageRange`,
    /// i.e., the difference between `virt_addr` and `self.start()`.
    pub fn offset_from_start(&self, virt_addr: VirtualAddress) -> Option<usize> {
        if self.contains_virt_addr(virt_addr) {
            Some(virt_addr.value() - self.start_address().value())
        } else {
            None
        }
    }
}

impl Deref for PageRange {
    type Target = RangeInclusive<Page>;
    fn deref(&self) -> &RangeInclusive<Page> {
        &self.0
    }
}
impl DerefMut for PageRange {
    fn deref_mut(&mut self) -> &mut RangeInclusive<Page> {
        &mut self.0
    }
}

impl IntoIterator for PageRange {
    type Item = Page;
    type IntoIter = RangeInclusive<Page>;

    fn into_iter(self) -> Self::IntoIter {
        self.0
    }
}


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
            let table = try!(temporary_page.map_table_frame(new_p4_frame.clone(), current_page_table));
            table.zero();

            #[cfg(any(target_arch="x86", target_arch="x86_64"))]
            let flags = EntryFlags::PRESENT | EntryFlags::WRITABLE;

            #[cfg(any(target_arch="aarch64"))]
            let flags = EntryFlags::PRESENT | EntryFlags::WRITABLE | EntryFlags::ACCESSEDARM | EntryFlags::INNER_SHARE;
                        
            table[RECURSIVE_P4_INDEX].set(new_p4_frame.clone(), flags);

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
        let mut backup = get_current_p4();
        if self.p4_table != backup {
            return Err("To invoke PageTable::with(), that PageTable ('self') must be currently active.");
        }

        // map temporary_page to current p4 table
        let p4_table = temporary_page.map_table_frame(backup.clone(), self)?;

        // overwrite recursive mapping
        #[cfg(any(target_arch="x86", target_arch="x86_64"))]
        {
            self.p4_mut()[RECURSIVE_P4_INDEX].set(other_table.p4_table.clone(), EntryFlags::PRESENT | EntryFlags::WRITABLE | EntryFlags::INNER_SHARE); 
            tlb::flush_all();
        }

        #[cfg(any(target_arch="aarch64"))]
        {
            self.p4_mut()[RECURSIVE_P4_INDEX].set(other_table.p4_table.clone(), EntryFlags::PRESENT | EntryFlags::WRITABLE | EntryFlags::INNER_SHARE | EntryFlags::ACCESSEDARM ); 
            tlb::flush_all();
        }

        // set mapper's target frame to reflect that future mappings will be mapped into the other_table
        self.mapper.target_p4 = other_table.p4_table.clone();

        // execute f in the new context
        let ret = f(self);

        // restore mapper's target frame to reflect that future mappings will be mapped using the currently-active (original) PageTable
        self.mapper.target_p4 = self.p4_table.clone();

        // // restore recursive mapping to original p4 table
        #[cfg(any(target_arch="x86", target_arch="x86_64"))]
        {
            p4_table[RECURSIVE_P4_INDEX].set(backup, EntryFlags::PRESENT | EntryFlags::WRITABLE);
            tlb::flush_all();
        }

        #[cfg(any(target_arch="aarch64"))] {
            p4_table[RECURSIVE_P4_INDEX].set(backup, EntryFlags::PRESENT | EntryFlags::PAGE | EntryFlags::INNER_SHARE | EntryFlags::ACCESSEDARM);
            tlb::flush_all();
        }

        // here, temporary_page is dropped, which auto unmaps it
        ret

    }


    /// Switches from the currently-active page table (this `PageTable`, i.e., `self`) to the given `new_table`.
    /// Returns the newly-switched-to PageTable.
    pub fn switch(&mut self, new_table: &PageTable) -> PageTable {
        // debug!("PageTable::switch() old table: {:?}, new table: {:?}", self, new_table);

        // perform the actual page table switch
        #[cfg(any(target_arch="x86", target_arch="x86_64"))]
        unsafe {
            control_regs::cr3_write(x86_64::PhysicalAddress(new_table.p4_table.start_address().value() as u64));
        }

        #[cfg(any(target_arch="aarch64"))] 
        unsafe {
          asm!("
            msr ttbr1_el1, x0;
            msr ttbr0_el1, x0;
            dsb ish; 
            isb; " : :"{x0}"(new_table.p4_table.start_address().value() as u64): : "volatile");
            tlb::flush_all();
        }
        let current_table_after_switch = PageTable::from_current();
        current_table_after_switch
    }


    /// Returns the physical address of this page table's top-level p4 frame
    pub fn physical_address(&self) -> PhysicalAddress {
        self.p4_table.start_address()
    }
}


/// Returns the current top-level page table frame, e.g., cr3 on x86
pub fn get_current_p4() -> Frame {
    #[cfg(any(target_arch="x86", target_arch="x86_64"))]
    { return Frame::containing_address(PhysicalAddress::new_canonical(control_regs::cr3().0 as usize)) }

    #[cfg(any(target_arch="aarch64"))]
    {   
        let p4:usize;
        unsafe {  asm!("mrs $0, TTBR0_EL1" : "=r"(p4) : : : "volatile"); };
        return Frame::containing_address(PhysicalAddress::new_canonical(p4))
    }
}

#[cfg(any(target_arch="aarch64"))]
pub fn enable_higher_half() {
unsafe {
    let p4_addr = get_current_p4().start_address().value() as u64;
    let recur_addr:*mut u64 = (p4_addr + RECURSIVE_P4_INDEX as u64 * 8) as *mut u64;
    let flags = EntryFlags::PRESENT | EntryFlags::PAGE | EntryFlags::INNER_SHARE | EntryFlags::ACCESSEDARM;

    *recur_addr = p4_addr | flags.bits();
    let level = 4;
    add_af_flag(p4_addr, level);
    asm!("
        dsb ish;
        isb;" : : : : "volatile");
    asm!("
        ldr x0, = 0x004404FF;
        msr mair_el1, x0;
        ldr x0, =0x00000005B5103510;
        msr tcr_el1, x0;
        isb;
        mrs x0, ttbr0_el1;
        msr ttbr1_el1, x0;
        dsb ish; 
        isb; 
        ldr x0, =0x0000000030d00801;
        msr sctlr_el1, x0;
        isb;" : : : : "volatile");
    };
    debug!("Enable higher half page table");          
}

#[cfg(any(target_arch="aarch64"))]
fn enable_temporary_page(allocator_mutex: &MutexIrqSafe<AreaFrameAllocator>) -> Result<(), &'static str>{
    unsafe {        
        let mut allocator = allocator_mutex.lock();

        let mut alloc_frame = || allocator.allocate_frame().ok_or("couldn't allocate frame");     

        let p3 = try!(alloc_frame());
        let p2 = try!(alloc_frame());
        let p1 = try!(alloc_frame());
        let p4 = get_current_p4();
        let flags = EntryFlags::PRESENT | EntryFlags::PAGE | EntryFlags::INNER_SHARE | EntryFlags::ACCESSEDARM;
        * ((p4.start_address().value() as u64 + KERNEL_TEXT_P4_INDEX as u64 * 8) as *mut u64) = p3.start_address().value() as u64 | flags.bits();
        * ((p3.start_address().value() as u64 + KERNEL_TEXT_P4_INDEX as u64 * 8) as *mut u64) =  p2.start_address().value() as u64 | flags.bits();
        * ((p2.start_address().value() as u64 + KERNEL_TEXT_P4_INDEX as u64 * 8) as *mut u64) =  p1.start_address().value() as u64 | flags.bits();
        asm!("
            dsb ish;
            isb;" : : : : "volatile");

        Ok(())
    }

}



#[cfg(any(target_arch="aarch64"))]
fn add_af_flag(p4_entry:u64, level:usize) {
    const ADDRESS_MASK:u64 = 0xfffffffffffff000;
    let p4_addr = p4_entry & ADDRESS_MASK;
    unsafe {
        for i in 0..super::ENTRIES_PER_PAGE_TABLE as u64 {
            let addr = (p4_addr + i * 8) as *mut u64;
            if *addr != 0 && (*addr & 0x400 == 0) {
                *addr = *addr | 0x400;
                if level > 2 {
                    add_af_flag(*addr, level - 1);
                }
            }
        }
    }
}


#[cfg(any(target_arch="aarch64"))]
pub fn set_recursive(p4_addr:u64) {
    debug!("Wenqiu: set recursive");
    unsafe { 
        let recur_addr:*mut u64 = (p4_addr + RECURSIVE_P4_INDEX as u64 * 8) as *mut u64;
        *recur_addr = p4_addr + 0x0703;
        asm!("
            dsb ish;
            isb;" : : : : "volatile");
    }
}


/// Initializes a new page table and sets up all necessary mappings for the kernel to continue running. 
/// Returns the following tuple, if successful:
/// 
///  * The kernel's new PageTable, which is now currently active,
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
#[cfg(any(target_arch="x86", target_arch="x86_64"))]
pub fn init(allocator_mutex: &MutexIrqSafe<AreaFrameAllocator>, boot_info: &multiboot2::BootInformation) 
    -> Result<(PageTable, Vec<VirtualMemoryArea>, MappedPages, MappedPages, MappedPages, Vec<MappedPages>, Vec<MappedPages>), &'static str>
{
    // bootstrap a PageTable from the currently-loaded page table
    let mut page_table = PageTable::from_current();

    let boot_info_start_vaddr = VirtualAddress::new(boot_info.start_address())?;
    let boot_info_start_paddr = page_table.translate(boot_info_start_vaddr).ok_or("Couldn't get boot_info start physical address")?;
    let boot_info_end_vaddr = VirtualAddress::new(boot_info.end_address())?;
    let boot_info_end_paddr = page_table.translate(boot_info_end_vaddr).ok_or("Couldn't get boot_info end physical address")?;
    let boot_info_size = boot_info.total_size();
    info!("multiboot start: {:#X}-->{:#X}, multiboot end: {:#X}-->{:#X}, size: {:#X}\n",
            boot_info_start_vaddr, boot_info_start_paddr, boot_info_end_vaddr, boot_info_end_paddr, boot_info_size
    );

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

    let elf_sections_tag = try!(boot_info.elf_sections_tag().ok_or("no Elf sections tag present!"));   
    let mut vmas: [VirtualMemoryArea; 32] = Default::default();
    let mut text_mapped_pages: Option<MappedPages> = None;
    let mut rodata_mapped_pages: Option<MappedPages> = None;
    let mut data_mapped_pages: Option<MappedPages> = None;
    let mut higher_half_mapped_pages: [Option<MappedPages>; 32] = Default::default();
    let mut identity_mapped_pages: [Option<MappedPages>; 32] = Default::default();

    // consumes and auto unmaps temporary page
    try!( page_table.with(&mut new_table, TemporaryPage::new(temp_frames2), |mapper| {
        
        // clear out the initially-mapped kernel entries of P4, since we're recreating kernel page tables from scratch.
        // (they were initialized in PageTable::new_table())
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

                if PhysicalAddress::new_canonical(section.start_address() as usize).frame_offset() != 0 {
                    error!("Section {} at {:#X}, size {:#X} was not page-aligned!", section.name(), section.start_address(), section.size());
                    return Err("Kernel ELF Section was not page-aligned");
                }

                let flags = EntryFlags::from_multiboot2_section_flags(&section) | EntryFlags::GLOBAL;

                // even though the linker stipulates that the kernel sections have a higher-half virtual address,
                // they are still loaded at a lower physical address, in which phys_addr = virt_addr - KERNEL_OFFSET.
                // thus, we must map the zeroeth kernel section from its low address to a higher-half address,
                // and we must map all the other sections from their higher given virtual address to the proper lower phys addr
                let mut start_phys_addr = section.start_address() as usize;
                if start_phys_addr >= KERNEL_OFFSET { 
                    // true for all sections but the first section (inittext)
                    start_phys_addr -= KERNEL_OFFSET;
                }
                
                let mut start_virt_addr = section.start_address() as usize;
                if start_virt_addr < KERNEL_OFFSET { 
                    // special case to handle the first section only
                    start_virt_addr += KERNEL_OFFSET;
                }

                let start_phys_addr = PhysicalAddress::new(start_phys_addr)?;
                let start_virt_addr = VirtualAddress::new(start_virt_addr)?;
                let end_virt_addr = start_virt_addr + (section.size() as usize);
                let end_phys_addr = start_phys_addr + (section.size() as usize);


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
                identity_mapped_pages[index] = Some(
                    mapper.map_frames(
                        FrameRange::from_phys_addr(start_phys_addr, section.size() as usize), 
                        Page::containing_address(start_virt_addr - KERNEL_OFFSET), 
                        flags,
                        allocator.deref_mut()
                    )?
                );
                debug!("           also mapped vaddr {:#X} to paddr {:#x} (size {:#X})", start_virt_addr - KERNEL_OFFSET, start_phys_addr, section.size());

                index += 1;      

            } // end of section iterator


            let (text_start_virt,    text_start_phys)    = text_start  .ok_or("Couldn't find start of .text section")?;
            let (_text_end_virt,     text_end_phys)      = text_end    .ok_or("Couldn't find end of .text section")?;
            let (rodata_start_virt,  rodata_start_phys)  = rodata_start.ok_or("Couldn't find start of .rodata section")?;
            let (_rodata_end_virt,   rodata_end_phys)    = rodata_end  .ok_or("Couldn't find end of .rodata section")?;
            let (data_start_virt,    data_start_phys)    = data_start  .ok_or("Couldn't find start of .data section")?;
            let (_data_end_virt,     data_end_phys)      = data_end    .ok_or("Couldn't find start of .data section")?;

            let text_flags    = text_flags  .ok_or("Couldn't find .text section flags")?;
            let rodata_flags  = rodata_flags.ok_or("Couldn't find .rodata section flags")?;
            let data_flags    = data_flags  .ok_or("Couldn't find .data section flags")?;


            // now we map the 5 main sections into 3 groups according to flags
            text_mapped_pages = Some( try!( mapper.map_frames(
                FrameRange::from_phys_addr(text_start_phys, text_end_phys.value() - text_start_phys.value()), 
                Page::containing_address(text_start_virt), 
                text_flags, allocator.deref_mut())
            ));
            rodata_mapped_pages = Some( try!( mapper.map_frames(
                FrameRange::from_phys_addr(rodata_start_phys, rodata_end_phys.value() - rodata_start_phys.value()), 
                Page::containing_address(rodata_start_virt), 
                rodata_flags, allocator.deref_mut())
            ));
            data_mapped_pages = Some( try!( mapper.map_frames(
                FrameRange::from_phys_addr(data_start_phys, data_end_phys.value() - data_start_phys.value()),
                Page::containing_address(data_start_virt), 
                data_flags, allocator.deref_mut())
            ));

            // map the VGA display memory as writable, which technically goes from 0xA_0000 - 0xC_0000 (exclusive),
            // VGA text mode only goes from 0xB_8000 - 0XC_0000
            const VGA_DISPLAY_PHYS_START: usize = 0xA_0000;
            const VGA_DISPLAY_PHYS_END: usize = 0xC_0000;
            const VGA_SIZE_IN_BYTES: usize = VGA_DISPLAY_PHYS_END - VGA_DISPLAY_PHYS_START;
            let vga_display_virt_addr = VirtualAddress::new_canonical(VGA_DISPLAY_PHYS_START + KERNEL_OFFSET);
            let vga_display_flags = EntryFlags::PRESENT | EntryFlags::WRITABLE | EntryFlags::GLOBAL | EntryFlags::NO_CACHE;
            higher_half_mapped_pages[index] = Some( try!( mapper.map_frames(
                FrameRange::from_phys_addr(PhysicalAddress::new(VGA_DISPLAY_PHYS_START)?, VGA_SIZE_IN_BYTES), 
                Page::containing_address(vga_display_virt_addr), 
                vga_display_flags,
                allocator.deref_mut())
            ));
            vmas[index] = VirtualMemoryArea::new(vga_display_virt_addr, VGA_SIZE_IN_BYTES, vga_display_flags, "Kernel VGA Display Memory");
            debug!("mapped kernel section: vga_buffer at addr: {:?}", vmas[index]);
            // also do an identity mapping for APs that need it while booting
            identity_mapped_pages[index] = Some( try!( mapper.map_frames(
                FrameRange::from_phys_addr(PhysicalAddress::new(VGA_DISPLAY_PHYS_START)?, VGA_SIZE_IN_BYTES), 
                Page::containing_address(VirtualAddress::new_canonical(VGA_DISPLAY_PHYS_START)), 
                vga_display_flags, allocator.deref_mut())
            ));
            index += 1;
            

            // map the multiboot boot_info at the same address it previously was, so we can continue to access boot_info 
            let boot_info_pages  = PageRange::from_virt_addr(boot_info_start_vaddr, boot_info_size);
            let boot_info_frames = FrameRange::from_phys_addr(boot_info_start_paddr, boot_info_size);
            vmas[index] = VirtualMemoryArea::new(boot_info_start_vaddr, boot_info_size, EntryFlags::PRESENT | EntryFlags::GLOBAL, "Kernel Multiboot Info");
            for (page, frame) in boot_info_pages.into_iter().zip(boot_info_frames) {
                // we must do it page-by-page to make sure that a page hasn't already been mapped
                if mapper.translate_page(page).is_some() {
                    // skip pages that are already mapped
                    continue;
                }
                higher_half_mapped_pages[index] = Some( try!( mapper.map_to(
                    page, frame.clone(), EntryFlags::PRESENT | EntryFlags::GLOBAL, allocator.deref_mut())
                ));
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
    let mut new_page_table = page_table.switch(&new_table); 
    // here, new_page_table and new_table should be identical
    debug!("switched to new page table {:?}.", new_page_table); 

    // After this point, we must "forget" all of the above mapped_pages instances if an error occurs,
    // because they will be auto-unmapped from the new page table upon return, causing all execution to stop.          


    // We must map the heap memory here, before it can initialized! 
    let (heap_mapped_pages, heap_vma) = {
        let mut allocator = allocator_mutex.lock();

        let pages = PageRange::from_virt_addr(VirtualAddress::new_canonical(KERNEL_HEAP_START), KERNEL_HEAP_INITIAL_SIZE);
        let heap_flags = paging::EntryFlags::WRITABLE;
        let heap_vma: VirtualMemoryArea = VirtualMemoryArea::new(VirtualAddress::new_canonical(KERNEL_HEAP_START), KERNEL_HEAP_INITIAL_SIZE, heap_flags, "Kernel Heap");
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
        (heap_mp, heap_vma)
    };

    debug!("mapped and initialized the heap, VMA: {:?}", heap_vma);
    // HERE: now the heap is set up, we can use dynamically-allocated types like Vecs

    let mut kernel_vmas: Vec<VirtualMemoryArea> = vmas.to_vec();
    kernel_vmas.retain(|x|  *x != VirtualMemoryArea::default() );
    kernel_vmas.push(heap_vma);

    debug!("kernel_vmas: {:?}", kernel_vmas);

    let mut higher_half: Vec<MappedPages> = higher_half_mapped_pages.iter_mut().filter_map(|opt| opt.take()).collect();
    higher_half.push(heap_mapped_pages);
    let identity: Vec<MappedPages> = identity_mapped_pages.iter_mut().filter_map(|opt| opt.take()).collect();

    // Return the new_page_table because that's the one that should be used by the kernel in future mappings. 
    Ok((new_page_table, kernel_vmas, text_mapped_pages, rodata_mapped_pages, data_mapped_pages, higher_half, identity))
}

#[cfg(any(target_arch="aarch64"))]
pub fn init(bt:&BootServices, allocator_mutex: &MutexIrqSafe<AreaFrameAllocator>, 
    phy_start:PhysicalAddress, phy_end:PhysicalAddress, stdout:&mut Output, image: uefi::Handle) 
   -> Result<(PageTable, Vec<VirtualMemoryArea>, MappedPages, MappedPages, MappedPages, Vec<MappedPages>, Vec<MappedPages>), &'static str> {
    //init higher half
    enable_higher_half();
    let p4_frame = get_current_p4();
    set_recursive(p4_frame.start_address().value() as u64);    
    
    let mut page_table = PageTable::from_current();

    // frame is a single frame, and temp_frames1/2 are tuples of 3 Frames each.
    let (new_frame, temp_frames1, temp_frames2) = {
        let mut allocator = allocator_mutex.lock();
        // a quick closure to allocate one frame
        let mut alloc_frame = || allocator.allocate_frame().ok_or("couldn't allocate frame");     
        (
            try!(alloc_frame()),
            (try!(alloc_frame()), try!(alloc_frame()), try!(alloc_frame())),
            (try!(alloc_frame()), try!(alloc_frame()), try!(alloc_frame()))
        )
    };

    try!(enable_temporary_page(allocator_mutex));

    let mut new_table = PageTable::new_table(&mut page_table, new_frame, TemporaryPage::new(temp_frames1))?;
    
    let mut vmas: [VirtualMemoryArea; 32] = Default::default();
    let mut text_mapped_pages: Option<MappedPages> = None;
    let mut rodata_mapped_pages: Option<MappedPages> = None;
    let mut data_mapped_pages: Option<MappedPages> = None;
    let mut higher_half_mapped_pages: [Option<MappedPages>; 32] = Default::default();
    let mut identity_mapped_pages: [Option<MappedPages>; 32] = Default::default();

    // consumes and auto unmaps temporary page
    try!( page_table.with(&mut new_table, TemporaryPage::new(temp_frames2), |mapper| {
        
        // clear out the initially-mapped kernel entries of P4, since we're recreating kernel page tables from scratch.
        // (they were initialized in InactivePageTable::new())
        //let p4 = mapper.p4_mut();

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
        
        let mut allocator = allocator_mutex.lock(); 

        let mut index = 0;    
        let mapped_info_size = bt.memory_map_size();
        let mapped_info_size = mapped_info_size + 8 * mem::size_of::<MemoryDescriptor>();
        
        let mut buffer = Vec::with_capacity(mapped_info_size);
        unsafe {
            buffer.set_len(mapped_info_size);
        }

        let (key, mut maps_iter) = bt
            .memory_map(&mut buffer)
            .expect_success("Failed to retrieve UEFI memory map");
        
        let mut kernel_phys_start: PhysicalAddress = PhysicalAddress::new(0)?;
        let mut kernel_phys_end: PhysicalAddress = PhysicalAddress::new(0)?;
        let mut avail_index = 0;
        let mut available: [PhysicalMemoryArea; 32] = Default::default();

        let mut occupied: [PhysicalMemoryArea; 32] = Default::default();
        let mut occup_index = 0;

        let mut mapped_pages_index = 0;

        const DEFAULT:usize = 0;
        const IMAGE_START:usize = 1;
        const UEFI_START:usize = 2;
        let mut address_section = DEFAULT;

        let mut index = 0;

        let flags = EntryFlags::GLOBAL;

        loop {

            match ( maps_iter.next()){
                Some(mapped_pages) => {
                    let start_phys_addr = mapped_pages.phys_start as usize;
                    let size = mapped_pages.page_count as usize * 0x1000;

                    if kernel_phys_start.value() == 0 {
                        kernel_phys_start = PhysicalAddress::new(start_phys_addr)?;
                    }
                
                    let end_phys_addr;
                    let end_virt_addr;
                    let start_virt_addr;
                    if start_phys_addr < 0xFFFFFFFFFFFFFFFF - KERNEL_OFFSET as usize {
                        start_virt_addr = start_phys_addr as usize + KERNEL_OFFSET;
                        end_virt_addr = VirtualAddress::new( start_virt_addr as usize + size )?;
                        end_phys_addr = PhysicalAddress::new( start_phys_addr as usize + size )?;
                    } else {
                        start_virt_addr = start_phys_addr as usize;
                        end_virt_addr = VirtualAddress::new( start_virt_addr as usize + size )?;
                        end_phys_addr = PhysicalAddress::new( start_phys_addr as usize + size )?;
                    }

                    let start_virt_addr = VirtualAddress::new(start_virt_addr as usize)?;         
                    let start_phys_addr = PhysicalAddress::new(start_phys_addr as usize)?;         
                    match mapped_pages.ty{

                         MemoryType::LOADER_DATA => {

                            if address_section == IMAGE_START {
                                data_start = Some((start_virt_addr, start_phys_addr));
                                data_end = Some((end_virt_addr, end_phys_addr));

                                identity_mapped_pages[index] = Some(try!( mapper.map_frames(
                                    FrameRange::from_phys_addr(start_phys_addr, size as usize), 
                                    Page::containing_address(start_virt_addr - KERNEL_OFFSET), 
                                        EntryFlags::NO_EXE_ARM | EntryFlags::PAGE, allocator.deref_mut())
                                ));
                                
                                vmas[index] = VirtualMemoryArea::new(start_virt_addr, size as usize, EntryFlags::GLOBAL, ".data");
                                index += 1;

                            }
                        },
                        MemoryType::LOADER_CODE => {
                            if address_section == IMAGE_START {
                                text_start = Some((start_virt_addr, start_phys_addr));
                                text_end = Some((end_virt_addr, end_phys_addr));
                                address_section = UEFI_START;
                                // This partion is not mapped as read-only because in the original mapping by UEFI, it is writable. 
                                // If map this partion as read-only, some UEFI services such as log does not work.
                                identity_mapped_pages[index] = Some(try!( mapper.map_frames(
                                    FrameRange::from_phys_addr(start_phys_addr, size as usize), 
                                    Page::containing_address(start_virt_addr - KERNEL_OFFSET), EntryFlags::PAGE, allocator.deref_mut())
                                ));
                                vmas[index] = VirtualMemoryArea::new(start_virt_addr, size as usize, EntryFlags::GLOBAL, ".data");
                                index += 1;
                            }
                        }
                        _ => {
                            if address_section == UEFI_START {
                                if  rodata_start.is_none() { 
                                    rodata_start = Some((start_virt_addr, start_phys_addr));
                                }
                                match rodata_end {
                                    Some((current_va, current_pa)) => {
                                        if current_pa < end_phys_addr {
                                            rodata_end = Some((end_virt_addr, end_phys_addr));
                                        } else {//MMIO
                                            //MMIO is handled together with other hardware resources later
                                        }
                                    },
                                    None => {
                                        rodata_end = Some((end_virt_addr, end_phys_addr));
                                    }
                                }
                            } else {
                                identity_mapped_pages[index] = Some(try!( mapper.map_frames(
                                                FrameRange::from_phys_addr(start_phys_addr, size as usize), 
                                                Page::containing_address(start_virt_addr - KERNEL_OFFSET), 
                                                EntryFlags::GLOBAL | EntryFlags::PAGE, allocator.deref_mut())
                                ));
                                vmas[index] = VirtualMemoryArea::new(start_virt_addr, size as usize, EntryFlags::GLOBAL, ".conventional");
                                index += 1;
                            }
                        }
                    }

                    if address_section != DEFAULT {
                    } else {
                        address_section = IMAGE_START;
                    }

                },
                None => break,
            }

            mapped_pages_index += 1;
        }

        //conventional
        //image data
        //image code
        //uefi
        //......
        //uefi
        //mmio
        //mmio

        debug!("WEnqiu: 1010101");
        let (text_start_virt,    text_start_phys)    = try!(text_start  .ok_or("Couldn't find start of .text section"));
        let (_text_end_virt,     text_end_phys)      = try!(text_end    .ok_or("Couldn't find end of .text section"));
        let (rodata_start_virt,  rodata_start_phys)  = try!(rodata_start.ok_or("Couldn't find start of .rodata section"));
        let (_rodata_end_virt,   rodata_end_phys)    = try!(rodata_end  .ok_or("Couldn't find end of .rodata section"));
        let (data_start_virt,    data_start_phys)    = try!(data_start  .ok_or("Couldn't find start of .data section"));
        let (_data_end_virt,     data_end_phys)      = try!(data_end    .ok_or("Couldn't find start of .data section"));

        identity_mapped_pages[index] = Some(try!( mapper.map_frames(
            FrameRange::from_phys_addr(rodata_start_phys,  (rodata_end_phys.value() - rodata_start_phys.value()) as usize), 
                Page::containing_address(rodata_start_virt - KERNEL_OFFSET), 
                EntryFlags::PAGE, allocator.deref_mut())
        ));
        vmas[index] = VirtualMemoryArea::new(rodata_start_virt, (rodata_start_phys.value() - rodata_end_phys.value()) as usize,
            EntryFlags::GLOBAL, ".uefi");
        index += 1;

        debug!("WEnqiu: 10101012");

        //Hardware resources https://github.com/qemu/qemu/blob/master/hw/arm/virt.c
        const HARDWARE_START:u64 = 0x1000 ;
        const HARDWARE_END:u64 = 0x40000000;
        let hardware_virt = VirtualAddress::new_canonical(HARDWARE_START as usize + 0xffff000000000000);
        debug!("WEnqiu: 1010104");
        identity_mapped_pages[index] = Some(try!( mapper.map_frames(
            FrameRange::from_phys_addr(PhysicalAddress::new(HARDWARE_START as usize)?,  (HARDWARE_END - HARDWARE_START) as usize), 
                Page::containing_address(hardware_virt), 
                EntryFlags::PAGE | EntryFlags::ACCESSEDARM | EntryFlags::INNER_SHARE, allocator.deref_mut())
        ));
        debug!("WEnqiu: 1010105s");
        vmas[index] = VirtualMemoryArea::new(hardware_virt, (HARDWARE_END - HARDWARE_START) as usize,
            EntryFlags::PAGE, ".mmio");
        index += 1;
        debug!("WEnqiu: 1010103");

        //Hardware resources https://github.com/qemu/qemu/blob/master/hw/arm/virt.c
        // const RESERVE_START:u64 = 0x0000008000000000 ;
        // const RESERVE_END:u64 = 0x0000010000000000;
        // let reserve_virt = VirtualAddress::new_canonical(RESERVE_START as usize + 0xffff000000000000);
        // debug!("WEnqiu: 1010104");
        // identity_mapped_pages[index] = Some(try!( mapper.map_frames(
        //     FrameRange::from_phys_addr(PhysicalAddress::new(RESERVE_START as usize)?,  (RESERVE_END - RESERVE_START) as usize), 
        //         Page::containing_address(reserve_virt), 
        //         EntryFlags::PAGE | EntryFlags::ACCESSEDARM | EntryFlags::INNER_SHARE, allocator.deref_mut())
        // ));
        // debug!("WEnqiu: 1010105s");
        // vmas[index] = VirtualMemoryArea::new(reserve_virt, (RESERVE_END - RESERVE_START) as usize,
        //     EntryFlags::PAGE, ".reserve");
        // index += 1;
        // debug!("WEnqiu: 1010103");

        // now we map the 5 main sections into 3 groups according to flags
        text_mapped_pages = Some( try!( mapper.map_frames(
            FrameRange::from_phys_addr(text_start_phys, text_end_phys.value() - text_start_phys.value()), 
            Page::containing_address(text_start_virt), 
            EntryFlags::PAGE | EntryFlags::ACCESSEDARM | EntryFlags::INNER_SHARE | EntryFlags::READONLY, allocator.deref_mut())
        ));
        rodata_mapped_pages = Some( try!( mapper.map_frames(
            FrameRange::from_phys_addr(rodata_start_phys, rodata_end_phys.value() - rodata_start_phys.value()), 
            Page::containing_address(rodata_start_virt), 
            EntryFlags::PAGE | EntryFlags::ACCESSEDARM | EntryFlags::INNER_SHARE |EntryFlags::READONLY, allocator.deref_mut())
        ));
        data_mapped_pages = Some( try!( mapper.map_frames(
            FrameRange::from_phys_addr(data_start_phys, data_end_phys.value() - data_start_phys.value()),
            Page::containing_address(data_start_virt), 
            EntryFlags::PAGE | EntryFlags::ACCESSEDARM | EntryFlags::INNER_SHARE, allocator.deref_mut())
        ));   
        debug!("WEnqiu: 10101011111");

        Ok(()) // mapping closure completed successfully

    })); // TemporaryPage is dropped here

    debug!("Wenqiu:5");

    let text_mapped_pages   = try!(text_mapped_pages  .ok_or("Couldn't map .text section"));
    let rodata_mapped_pages = try!(rodata_mapped_pages.ok_or("Couldn't map .rodata section"));
    let data_mapped_pages   = try!(data_mapped_pages  .ok_or("Couldn't map .data section"));

    debug!("switching to new page table {:?}", new_table);
    let mut new_page_table = page_table.switch(&new_table); 
    // here, new_page_table and new_table should be identical
    debug!("switched to new page table {:?}.", new_page_table); 

    // After this point, we must "forget" all of the above mapped_pages instances if an error occurs,
    // because they will be auto-unmapped from the new page table upon return, causing all execution to stop.          


    // We must map the heap memory here, before it can initialized! 
    let (heap_mapped_pages, heap_vma) = {
        let mut allocator = allocator_mutex.lock();

        let pages = PageRange::from_virt_addr(VirtualAddress::new_canonical(KERNEL_HEAP_START), KERNEL_HEAP_INITIAL_SIZE);
        let heap_flags = paging::EntryFlags::WRITABLE;
        let heap_vma: VirtualMemoryArea = VirtualMemoryArea::new(VirtualAddress::new_canonical(KERNEL_HEAP_START), KERNEL_HEAP_INITIAL_SIZE, heap_flags, "Kernel Heap");
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

    Ok((new_page_table, kernel_vmas, text_mapped_pages, rodata_mapped_pages, data_mapped_pages, higher_half, identity))
}



// /// Get a stack trace, borrowed from Redox
// /// TODO: Check for stack being mapped before dereferencing
// #[inline(never)]
// pub fn stack_trace() {
//     use core::mem;

//     // SAFE, just a stack trace for debugging purposes, and pointers are checked. 
//     unsafe {
        
//         // get the stack base pointer
//         let mut rbp: usize;
//         asm!("" : "={rbp}"(rbp) : : : "intel", "volatile");

//         error!("STACK TRACE: {:>016X}", rbp);
//         //Maximum 64 frames
//         let page_table = PageTable::from_current();
//         for _frame in 0..64 {
//             if let Some(rip_rbp) = rbp.checked_add(mem::size_of::<usize>()) {
//                 // TODO: is this the right condition?
//                 match (VirtualAddress::new(rbp), VirtualAddress::new(rip_rbp)) {
//                     (Ok(rbp_vaddr), Ok(rip_rbp_vaddr)) => {
//                         if page_table.translate(rbp_vaddr).is_some() && page_table.translate(rip_rbp_vaddr).is_some() {
//                             let rip = *(rip_rbp as *const usize);
//                             if rip == 0 {
//                                 error!(" {:>016X}: EMPTY RETURN", rbp);
//                                 break;
//                             }
//                             error!("  {:>016X}: {:>016X}", rbp, rip);
//                             rbp = *(rbp as *const usize);
//                             // symbol_trace(rip);
//                         } else {
//                             error!("  {:>016X}: GUARD PAGE", rbp);
//                             break;
//                         }
//                     }
//                     _ => {
//                         error!(" {:>016X}: INVALID_ADDRESS", rbp);
//                         break;
//                     }
//                 }
                
//             } else {
//                 error!("  {:>016X}: RBP OVERFLOW", rbp);
//             }
//         }
//     }
// }
