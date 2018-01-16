// Copyright 2016 Philipp Oppermann. See the README.md
// file at the top-level directory of this distribution.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use alloc::BTreeSet;
use mod_mgmt::find_first_section_by_type;
pub use self::entry::*;
use memory::{Frame, FrameAllocator};
pub use self::temporary_page::TemporaryPage;
pub use self::mapper::Mapper;
use core::ops::{Add, AddAssign, Sub, SubAssign, Deref, DerefMut};
use multiboot2;
use super::*; //{MAX_MEMORY_AREAS, VirtualMemoryArea};

use x86_64::registers::control_regs;
use x86_64::instructions::tlb;

use kernel_config::memory::{PAGE_SIZE, MAX_PAGE_NUMBER, RECURSIVE_PAGE_TABLE_INDEX};
use kernel_config::memory::{KERNEL_TEXT_P4_INDEX, KERNEL_HEAP_P4_INDEX, KERNEL_STACK_P4_INDEX};

mod entry;
mod table;
mod temporary_page;
mod mapper;



#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Page {
    number: usize,
}

impl Page {
	/// returns the first virtual address as the start of this Page
    pub fn containing_address(address: VirtualAddress) -> Page {
        assert!(address < 0x0000_8000_0000_0000 || address >= 0xffff_8000_0000_0000,
                "Page::containing_address(): invalid address: 0x{:x}", address);
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
        PageIter {
            start: start,
            end: end,
        }
    }

    pub fn range_inclusive_addr(virt_addr: VirtualAddress, size_in_bytes: usize) -> PageIter {
        let start_page = Page::containing_address(virt_addr);
        let end_page = Page::containing_address(virt_addr + size_in_bytes - 1);
        PageIter {
            start: start_page,
            end: end_page,
        }
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


#[derive(Clone)]
pub struct PageIter {
    start: Page,
    end: Page,
}

impl Iterator for PageIter {
    type Item = Page;

    fn next(&mut self) -> Option<Page> {
        if self.start <= self.end {
            let page = self.start;
            self.start.number += 1;
            Some(page)
        } else {
            None
        }
    }
}

/// the owner of the recursively defined P4 page table. 
pub struct ActivePageTable {
    mapper: Mapper,
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
    unsafe fn new() -> ActivePageTable {
        ActivePageTable { mapper: Mapper::new() }
    }

    /// temporarily maps the given `InactivePageTable` to the recursive entry (510th entry) 
    /// so that we can set up new mappings on the new `table` before actually switching to it.
    /// THIS DOES NOT PERFORM ANY CONTEXT SWITCHING OR CHANGING OF THE CURRENT PAGE TABLE REGISTER (e.g., CR3)
    pub fn with<F>(&mut self,
                   table: &mut InactivePageTable,
                   temporary_page: &mut temporary_page::TemporaryPage,
                   f: F)
        where F: FnOnce(&mut Mapper)
    {

        {
            let backup = Frame::containing_address(control_regs::cr3().0 as usize);

            // map temporary_page to current p4 table
            let p4_table = temporary_page.map_table_frame(backup.clone(), self);

            // overwrite recursive mapping
            self.p4_mut()[RECURSIVE_PAGE_TABLE_INDEX].set(table.p4_frame.clone(), EntryFlags::PRESENT | EntryFlags::WRITABLE); 
            tlb::flush_all();

            // execute f in the new context
            f(self);

            // restore recursive mapping to original p4 table
            p4_table[RECURSIVE_PAGE_TABLE_INDEX].set(backup, EntryFlags::PRESENT | EntryFlags::WRITABLE);
            tlb::flush_all();
        }

        temporary_page.unmap(self);
    }

    /// returns the old_table as an InactivePageTable, and the newly-created ActivePageTable.
    // pub fn switch(&mut self, new_table: &InactivePageTable) -> InactivePageTable {
    pub fn switch(&mut self, new_table: &InactivePageTable) -> (InactivePageTable, ActivePageTable) {
        use x86_64::PhysicalAddress;

        let old_table = InactivePageTable {
            p4_frame: Frame::containing_address(control_regs::cr3().0 as usize),
        };
        unsafe {
            control_regs::cr3_write(PhysicalAddress(new_table.p4_frame.start_address() as u64));
        }
        
        // debug!("ActivePageTable::switch(): NEW TABLE!!!");

        // old_table
        (old_table, unsafe { ActivePageTable::new() } )
    }


    /// Returns the physical address of this page table's top-level entry,
    /// e.g., the value of the CR3 register on x86
    pub fn physical_address(&self) -> PhysicalAddress {
        control_regs::cr3().0 as usize as PhysicalAddress
    }
}




pub struct InactivePageTable {
    p4_frame: Frame,
}

impl InactivePageTable {
    pub fn new(frame: Frame,
               active_table: &mut ActivePageTable,
               temporary_page: &mut TemporaryPage)
               -> InactivePageTable {
        {
            let table = temporary_page.map_table_frame(frame.clone(), active_table);
            table.zero();

            table[RECURSIVE_PAGE_TABLE_INDEX].set(frame.clone(), EntryFlags::PRESENT | EntryFlags::WRITABLE);

            // start out by copying all the kernel sections into the new inactive table
            table.copy_entry_from_table(active_table.p4(), KERNEL_TEXT_P4_INDEX);
            table.copy_entry_from_table(active_table.p4(), KERNEL_HEAP_P4_INDEX);
            table.copy_entry_from_table(active_table.p4(), KERNEL_STACK_P4_INDEX);
        }
        temporary_page.unmap(active_table);

        InactivePageTable { p4_frame: frame }
    }
}


pub enum PageTable {
    Uninitialized,
    Active(ActivePageTable),
    Inactive(InactivePageTable),
}



pub fn remap_the_kernel<A>(allocator: &mut A, 
    boot_info: &multiboot2::BootInformation, 
    vmas: &mut [VirtualMemoryArea; 32]) 
    -> Result<ActivePageTable, &'static str>
    where A: FrameAllocator
{
    //  let mut temporary_page = TemporaryPage::new(Page { number: 0xcafebabe }, allocator);
    // the temporary page uses the very last address of the kernel heap, so it'll pretty much never collide
    let mut temporary_page = TemporaryPage::new(allocator);

    let mut active_table: ActivePageTable = unsafe { ActivePageTable::new() };
    let mut new_table: InactivePageTable = {
        let frame = try!(allocator.allocate_frame().ok_or("couldn't allocate frame"));
        InactivePageTable::new(frame, &mut active_table, &mut temporary_page)
    };

    let elf_sections_tag = try!(boot_info.elf_sections_tag().ok_or("no Elf sections tag present!"));

    active_table.with(&mut new_table, &mut temporary_page, |mapper| {
        // clear out the initially-mapped kernel entries of P4
        // (they are initialized in InactivePageTable::new())
        mapper.p4_mut().clear_entry(KERNEL_TEXT_P4_INDEX);
        mapper.p4_mut().clear_entry(KERNEL_HEAP_P4_INDEX);
        mapper.p4_mut().clear_entry(KERNEL_STACK_P4_INDEX);

        // map the allocated kernel text sections
        let mut index = 0;
        for section in elf_sections_tag.sections() {
            if section.size() == 0 || !section.is_allocated() {
                // skip sections that aren't loaded to memory
                continue;
            }

            assert!(section.start_address() as usize % PAGE_SIZE == 0,
                    "sections need to be page aligned");

            let flags = EntryFlags::from_multiboot2_section_flags(&section) | EntryFlags::GLOBAL;

            // even though the linker stipulates that the kernel sections have a higher-half virtual address,
            // they are still loaded at a lower physical address, in which phys_addr = virt_addr - KERNEL_OFFSET.
            // thus, we must map the zeroeth kernel section from its low address to a higher-half address,
            // and we must map all the other sections from their higher given virtual address to the proper lower phys addr
            let mut start_phys_addr = section.start_address() as PhysicalAddress;
            if start_phys_addr >= super::KERNEL_OFFSET { 
                // true for all sections but the first section (inittext)
                start_phys_addr -= super::KERNEL_OFFSET;
            }
            
            let mut start_virt_addr = section.start_address() as VirtualAddress;
            if start_virt_addr < super::KERNEL_OFFSET { 
                // special case to handle the first section only
                start_virt_addr += super::KERNEL_OFFSET;
            }

            vmas[index] = VirtualMemoryArea::new(start_virt_addr, section.size() as usize, flags, "KERNEL ELF SECTION TODO FIXME");
            debug!("mapping kernel section: {} at addr: {:?}", section.name(), vmas[index]);
            
            // map the whole range of frames in this section
            mapper.map_frames(Frame::range_inclusive_addr(start_phys_addr, section.size() as usize), 
                              Page::containing_address(start_virt_addr), 
                              flags, allocator);

            index += 1;
        }

        // let's just go ahead and map the entire first megabyte of physical memory,
        // which happens to include ACPI tables, VGA memory, etc
        // (0x0 - 0x10_0000) => (0xFFFF_FFFF_8000_0000 - 0xFFFF_FFFF_8010_0000)
        mapper.map_frames(Frame::range_inclusive_addr(0x0, 0x10_0000), 
                          Page::containing_address(KERNEL_OFFSET as VirtualAddress), 
                          EntryFlags::PRESENT | EntryFlags::GLOBAL, allocator);
        vmas[index] = VirtualMemoryArea::new(KERNEL_OFFSET, 0x10_0000, EntryFlags::PRESENT | EntryFlags::GLOBAL, "Kernel low memory (BIOS)");
        index += 1;

        // remap the VGA display memory as writable, which goes from 0xA_0000 - 0xC_0000 (exclusive)
        // but currently we're only using VGA text mode, which goes from 0xB_8000 - 0XC_0000
        const VGA_DISPLAY_PHYS_START: PhysicalAddress = 0xB_8000;
        const VGA_DISPLAY_PHYS_END: PhysicalAddress = 0xC_0000;
        let vga_display_virt_addr: VirtualAddress = VGA_DISPLAY_PHYS_START + KERNEL_OFFSET;
        let size_in_bytes: usize = VGA_DISPLAY_PHYS_END - VGA_DISPLAY_PHYS_START;
        let vga_display_flags = EntryFlags::WRITABLE | EntryFlags::GLOBAL | EntryFlags::NO_CACHE;
        vmas[index] = VirtualMemoryArea::new(vga_display_virt_addr, size_in_bytes, vga_display_flags, "Kernel VGA Display Memory");
        // use remap because we already mapped it above
        mapper.remap_pages(Page::range_inclusive_addr(vga_display_virt_addr, size_in_bytes), vga_display_flags);


        // unmap the kernel's original identity mapping (including multiboot2 boot_info) to clear the way for userspace mappings
        // ACTUALLY we cannot do this until we have booted up all the APs
        // mapper.p4_mut().clear_entry(0);
    });


    let (_old_table, new_active_table) = active_table.switch(&new_table);

    // Return the new_active_table because that's the one that should be used by the kernel (task_zero) in future mappings. 
    Ok(new_active_table)
}