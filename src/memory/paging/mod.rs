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



    // pub fn switch_to_higher_half(&mut self, new_cr3: u64, function_jump: usize) -> ! {
    //     use x86_64::PhysicalAddress;

    //     unsafe {
    //         asm!("mov $0, %cr3" :: "r" (new_cr3) : "memory");
    //         asm!("jmp $0" : : "r"(function_jump) : "memory" : "intel", "volatile");
    //     }

    //     loop { }
    // }

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

    let mut active_table = unsafe { ActivePageTable::new() };
    let mut new_table = {
        let frame = allocator.allocate_frame().expect("no more frames");
        InactivePageTable::new(frame, &mut active_table, &mut temporary_page)
    };

    active_table.with(&mut new_table, &mut temporary_page, |mapper| {
        let elf_sections_tag = boot_info.elf_sections_tag().expect("Memory map tag required");

        // map the allocated kernel text sections
        // we're no longer using identity maps, but rather a linear offset 
        // in which VirtualAddress = PhysicalAddress + KERNEL_OFFSET
        for section in elf_sections_tag.sections() {
            if !section.is_allocated() {
                // skip sections that aren't loaded to memory
                continue;
            }

            assert!(section.addr as usize % PAGE_SIZE == 0,
                    "sections need to be page aligned");
            println_unsafe!("mapping section at addr: {:#x}, size: {:#x}",
                     section.addr,
                     section.size);

            let mut flags = EntryFlags::from_elf_section_flags(section);

            let start_frame = Frame::containing_address(section.start_address());
            let end_frame = Frame::containing_address(section.end_address() - 1);
            for frame in Frame::range_inclusive(start_frame, end_frame) {
                // TEMPORARY HACK:  is there a better way to determine which
                //                  kernel sections should be mapped to higher half?
                if section.addr >= (super::KERNEL_OFFSET as u64) {
                    // this is the common case and will be true for all but the first section,
                    // because our linker ld script already stipulates that all but the first inittext/boot
                    // section will be mapped to higher half (phys_addr + KERNEL_OFFSET)
                    mapper.identity_map(frame, flags, allocator);
                }
                else {
                    mapper.map_linear_offset(frame.clone(), super::KERNEL_OFFSET, flags, allocator);
                }
            }
        }

        // linear map the VGA text buffer to 0xb8000 + KERNEL_OFFSET
        let vga_buffer_frame = Frame::containing_address(0xb8000);
        mapper.map_linear_offset(vga_buffer_frame.clone(), super::KERNEL_OFFSET, WRITABLE, allocator);
        mapper.identity_map(vga_buffer_frame, WRITABLE, allocator);

        // linear map the multiboot info structure
        // FIXME: we don't need this anymore because we're not using it (it's copied into PHYSICAL_MEMORY_AREAS)
        // let multiboot_start = Frame::containing_address(boot_info.start_address());
        // let multiboot_end = Frame::containing_address(boot_info.end_address() - 1);
        // for frame in Frame::range_inclusive(multiboot_start, multiboot_end) {
        //     mapper.map_linear_offset(frame.clone(), super::KERNEL_OFFSET, PRESENT, allocator);
        //     mapper.identity_map(frame, PRESENT, allocator);
        // }
    });

    // active_table.switch_to_higher_half(new_table.p4_frame.start_address() as u64, higher_half_entry as usize);

    let old_table = active_table.switch(new_table);
    println_unsafe!("NEW TABLE!!!");


    unsafe {
        *((0xb8000 + super::KERNEL_OFFSET) as *mut u64) = 0x2f592f412f4b2f4f;
    }

    loop {}

    let old_p4_page = Page::containing_address(old_table.p4_frame.start_address());
    active_table.unmap(old_p4_page, allocator);
    println_unsafe!("guard page at {:#x}", old_p4_page.start_address());

    active_table
}


