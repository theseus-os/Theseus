// Copyright 2016 Philipp Oppermann. See the README.md
// file at the top-level directory of this distribution.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

pub use self::entry::*;
use memory::{PAGE_SIZE, Frame, FrameAllocator};
use self::temporary_page::TemporaryPage;
pub use self::mapper::Mapper;
use core::ops::{Add, Deref, DerefMut};
use multiboot2::BootInformation;

mod entry;
mod table;
mod temporary_page;
mod mapper;

const ENTRY_COUNT: usize = 512;
const RECURSIVE_INDEX: usize = 510;

pub type PhysicalAddress = usize;
pub type VirtualAddress = usize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Page {
    number: usize,
}

impl Page {
	/// returns the first virtual address as the start of this Page
    pub fn containing_address(address: VirtualAddress) -> Page {
        assert!(address < 0x0000_8000_0000_0000 || address >= 0xffff_8000_0000_0000,
                "invalid address: 0x{:x}",
                address);
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
}

impl Add<usize> for Page {
    type Output = Page;

    fn add(self, rhs: usize) -> Page {
        Page { number: self.number + rhs }
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

    pub fn with<F>(&mut self,
                   table: &mut InactivePageTable,
                   temporary_page: &mut temporary_page::TemporaryPage, // new
                   f: F)
        where F: FnOnce(&mut Mapper)
    {
        use x86_64::registers::control_regs;
        use x86_64::instructions::tlb;

        {
            let backup = Frame::containing_address(control_regs::cr3().0 as usize);

            // map temporary_page to current p4 table
            let p4_table = temporary_page.map_table_frame(backup.clone(), self);

            // overwrite recursive mapping
            self.p4_mut()[RECURSIVE_INDEX].set(table.p4_frame.clone(), PRESENT | WRITABLE);
            tlb::flush_all();

            // execute f in the new context
            f(self);

            // restore recursive mapping to original p4 table
            p4_table[RECURSIVE_INDEX].set(backup, PRESENT | WRITABLE);
            tlb::flush_all();
        }

        temporary_page.unmap(self);
    }

    pub fn switch(&mut self, new_table: InactivePageTable) -> InactivePageTable {
        use x86_64::PhysicalAddress;
        use x86_64::registers::control_regs;

        let old_table = InactivePageTable {
            p4_frame: Frame::containing_address(control_regs::cr3().0 as usize),
        };
        unsafe {
            control_regs::cr3_write(PhysicalAddress(new_table.p4_frame.start_address() as u64));
        }
        old_table
    }

}


// pub fn higher_half_entry() {

//     unsafe {
//         *((0xb8000 + super::KERNEL_OFFSET) as *mut u64) = 0x2f592f412f4b2f4f;
//     }
//     loop { }
// }



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
            table[RECURSIVE_INDEX].set(frame.clone(), PRESENT | WRITABLE);
        }
        temporary_page.unmap(active_table);

        InactivePageTable { p4_frame: frame }
    }
}

pub fn remap_the_kernel<A>(allocator: &mut A, boot_info: &BootInformation) -> ActivePageTable
    where A: FrameAllocator
{
     let mut temporary_page = TemporaryPage::new(Page { number: 0xcafebabe }, allocator);
    //let mut temporary_page = TemporaryPage::new(Page::containing_address(0xFFFF_FFFF_FFFF_FFF0), allocator);

    let mut active_table: ActivePageTable = unsafe { ActivePageTable::new() };
    let mut new_table: InactivePageTable = {
        let frame = allocator.allocate_frame().expect("no more frames");
        InactivePageTable::new(frame, &mut active_table, &mut temporary_page)
    };

    active_table.with(&mut new_table, &mut temporary_page, |mapper| {
        let elf_sections_tag = boot_info.elf_sections_tag().expect("Elf sections tag required");

        // map the allocated kernel text sections
        for section in elf_sections_tag.sections() {
            if section.size == 0 || !section.is_allocated() {
                // skip sections that aren't loaded to memory
                continue;
            }

            assert!(section.addr as usize % PAGE_SIZE == 0,
                    "sections need to be page aligned");
            println_unsafe!("mapping section at addr: {:#x}, size: {:#x}",
                     section.addr,
                     section.size);

            let mut flags = EntryFlags::from_elf_section_flags(section);

            // even though the linker stipulates that the kernel sections have a higher-half virtual address,
            // they are still loaded at a lower physical address, in which phys_addr = virt_addr - KERNEL_OFFSET.
            // thus, we must map the zeroeth kernel section from its low address to a higher-half address,
            // and we must map all the other sections from their higher given virtual address to the proper lower phys addr
            let mut start_phys_addr = section.start_address();
            if start_phys_addr >= super::KERNEL_OFFSET { 
                // true for all sections but the first section (inittext)
                start_phys_addr -= super::KERNEL_OFFSET;
            }
            
            let mut start_virt_addr = section.start_address();
            if start_virt_addr < super::KERNEL_OFFSET { 
                // special case to handle the first section only
                start_virt_addr += super::KERNEL_OFFSET;
            }

            // map the whole range of pages to frames in this section
            mapper.map_contiguous_range(start_virt_addr, start_phys_addr, section.size as usize, flags, allocator);
        }

        // map the VGA text buffer to 0xb8000 + KERNEL_OFFSET
        let vga_buffer_frame = Frame::containing_address(0xb8000);
        mapper.map_virtual_address(0xb8000 + super::KERNEL_OFFSET, vga_buffer_frame, WRITABLE, allocator);
    });


    let old_table = active_table.switch(new_table);
    println_unsafe!("NEW TABLE!!!");


    // unsafe {
    //     *((0xb8000 + super::KERNEL_OFFSET) as *mut u64) = 0x2f592f412f4b2f4f;
    // }

    // loop {}

    // DEPRECATED:  the boot.S file sets up the guard page by zero-ing pml4t and pmdp in start64_high
    // let old_p4_page = Page::containing_address(old_table.p4_frame.start_address());
    // active_table.unmap(old_p4_page, allocator);
    // println_unsafe!("guard page at {:#x}", old_p4_page.start_address());

    active_table // now it's the old table
}


