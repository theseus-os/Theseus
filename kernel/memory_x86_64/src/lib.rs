//! This crate implements the virtual memory subsystem interfaces for Theseus on x86_64.
//! `memory` uses this crate to get the memory layout and do other arch-specific operations on x86_64.  
//! 
//! This is the top-level arch-specific memory crate. 
//! All arch-specific definitions for memory system are exported from this crate.

#![no_std]
#![feature(ptr_internals)]
#![feature(unboxed_closures)]

#[macro_use] extern crate log;
extern crate kernel_config;
extern crate memory_structs;
extern crate entryflags_x86_64;
extern crate x86_64;
extern crate boot_info;

pub use entryflags_x86_64::{EntryFlags, PAGE_TABLE_ENTRY_FRAME_MASK};

use boot_info::{ElfSection, ElfSectionFlags};
use kernel_config::memory::KERNEL_OFFSET;
use memory_structs::{
    PhysicalAddress, VirtualAddress, SectionMemoryBounds, AggregatedSectionMemoryBounds,
};
use x86_64::{registers::control::Cr3, instructions::tlb};


/// Finds the addresses in memory of the main kernel sections, as specified by the given boot information. 
/// 
/// Returns the following tuple, if successful:
///  * The combined size and address bounds of key sections, e.g., .text, .rodata, .data.
///    Each of the these section bounds is aggregated to cover the bounds and sizes of *all* sections 
///    that share the same page table mapping flags and can thus be logically combined.
///  * The list of all individual sections found. 
pub fn find_section_memory_bounds<T>(boot_info: &T) -> Result<(AggregatedSectionMemoryBounds, [Option<SectionMemoryBounds>; 32]), &'static str>
where
    T: boot_info::BootInformation
{
    let elf_sections_tag = boot_info.elf_sections()?;

    let mut index = 0;
    let mut text_start:        Option<(VirtualAddress, PhysicalAddress)> = None;
    let mut text_end:          Option<(VirtualAddress, PhysicalAddress)> = None;
    let mut rodata_start:      Option<(VirtualAddress, PhysicalAddress)> = None;
    let mut rodata_end:        Option<(VirtualAddress, PhysicalAddress)> = None;
    let mut data_start:        Option<(VirtualAddress, PhysicalAddress)> = None;
    let mut data_end:          Option<(VirtualAddress, PhysicalAddress)> = None;

    let mut text_flags:        Option<EntryFlags> = None;
    let mut rodata_flags:      Option<EntryFlags> = None;
    let mut data_flags:        Option<EntryFlags> = None;

    let mut sections_memory_bounds: [Option<SectionMemoryBounds>; 32] = Default::default();

    // map the allocated kernel text sections
    for section in elf_sections_tag {
        // skip sections that don't need to be loaded into memory
        if section.len() == 0
            || !section.flags().contains(boot_info::ElfSectionFlags::ALLOCATED)
            || section.name().starts_with(".debug")
        {
            continue;
        }

        debug!("Looking at loaded section {} at {:#X}, size {:#X}", section.name(), section.start(), section.len());

        let mut flags = EntryFlags::GLOBAL;
        if section.flags().contains(ElfSectionFlags::ALLOCATED) {
            // Section is loaded to memory
            flags |= EntryFlags::PRESENT;
        }
        if section.flags().contains(ElfSectionFlags::WRITABLE) {
            flags |= EntryFlags::WRITABLE;
        }
        if !section.flags().contains(ElfSectionFlags::EXECUTABLE) {
            flags |= EntryFlags::NO_EXECUTE;
        }

        // even though the linker stipulates that the kernel sections have a higher-half virtual address,
        // they are still loaded at a lower physical address, in which phys_addr = virt_addr - KERNEL_OFFSET.
        // thus, we must map the zeroeth kernel section from its low address to a higher-half address,
        // and we must map all the other sections from their higher given virtual address to the proper lower phys addr
        let mut start_phys_addr = section.start().value();
        if start_phys_addr >= KERNEL_OFFSET {
            // true for all sections but the first section (inittext)
            start_phys_addr -= KERNEL_OFFSET;
        }

        let mut start_virt_addr = section.start().value();
        if start_virt_addr < KERNEL_OFFSET {
            // special case to handle the first section only
            start_virt_addr += KERNEL_OFFSET;
        }

        let start_phys_addr = PhysicalAddress::new(start_phys_addr).ok_or("section had invalid starting physical address")?;
        let start_virt_addr = VirtualAddress::new(start_virt_addr).ok_or("section had invalid ending physical address")?;
        let end_virt_addr = start_virt_addr + section.len();
        let end_phys_addr = start_phys_addr + section.len();

        // The linker script (linker_higher_half.ld) defines the following order of sections:
        // |------|-------------------|-------------------------------|
        // | Sec  |    Sec Name       |    Description / purpose      |
        // | Num  |                   |                               |
        // |------|---------------------------------------------------|
        // | (1)  | .init             | start of executable pages     |
        // | (2)  | .text             | end of executable pages       |
        // | (3)  | .rodata           | start of read-only pages      |
        // | (4)  | .eh_frame         | part of read-only pages       |
        // | (5)  | .gcc_except_table | part/end of read-only pages   |
        // | (6)  | .tdata            | part/end of read-only pages   |
        // | (7)  | .tbss             | part/end of read-only pages   |
        // | (8)  | .data             | start of read-write pages     | 
        // | (9)  | .bss              | end of read-write pages       |
        // | (10) | .page_table       | separate .data-like section   |
        // | (11) | .stack            | separate .data-like section   |
        // |------|-------------------|-------------------------------|
        //
        // Note that we combine the TLS data sections (.tdata and .tbss) into the read-only pages,
        // because they contain read-only initializer data "images" for each TLS area.
        // In fact, .tbss can be completedly ignored because it represents a read-only data image of all zeroes,
        // so there's no point in keeping it around.
        //
        // Those are the only sections we care about; we ignore subsequent `.debug_*` sections (and .got).
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
                "nano_core .rodata"
            }
            ".eh_frame" => {
                "nano_core .eh_frame"
            }
            ".gcc_except_table" => {
                rodata_end   = Some((end_virt_addr, end_phys_addr));
                rodata_flags = Some(flags);
                "nano_core .gcc_except_table"
            }
            // The following four sections are optional: .tdata, .tbss, .data, .bss.
            ".tdata" => {
                rodata_end = Some((end_virt_addr, end_phys_addr));
                "nano_core .tdata"
            }
            ".tbss" => {
                // Ignore .tbss (see above) because it is a read-only section of all zeroes.
                debug!("     no need to map kernel section \".tbss\", it contains no content");
                continue;
            }
            ".data" => {
                data_start.get_or_insert((start_virt_addr, start_phys_addr));
                data_end = Some((end_virt_addr, end_phys_addr));
                data_flags = Some(flags);
                "nano_core .data"
            }
            ".bss" => {
                data_start.get_or_insert((start_virt_addr, start_phys_addr));
                data_end = Some((end_virt_addr, end_phys_addr));
                data_flags = Some(flags);
                "nano_core .bss"
            }
            // This appears when compiling for BIOS.
            ".page_table" | ".stack" => {
                continue;
            }
            // This appears when compiling for UEFI.
            ".bootloader-config" => {
                // TODO: Ideally we'd mark bootloader-config as not allocated
                // so the bootloader doesn't load it.
                continue;
            }
            _ =>  {
                error!("Section {} at {:#X}, size {:#X} was not an expected section", 
                        section.name(), section.start(), section.len());
                return Err("Kernel ELF Section had an unexpected name");
            }
        };
        debug!("     will map kernel section {:?} as {:?} at vaddr: {:#X}, size {:#X} bytes", section.name(), static_str_name, start_virt_addr, section.len());

        sections_memory_bounds[index] = Some(SectionMemoryBounds {
            start: (start_virt_addr, start_phys_addr),
            end: (end_virt_addr, end_phys_addr),
            flags,
        });

        index += 1;
    }

    let text_start         = text_start       .ok_or("Couldn't find start of .text section")?;
    let text_end           = text_end         .ok_or("Couldn't find end of .text section")?;
    let rodata_start       = rodata_start     .ok_or("Couldn't find start of .rodata section")?;
    let rodata_end         = rodata_end       .ok_or("Couldn't find end of .rodata section")?;
    let data_start         = data_start       .ok_or("Couldn't find start of .data section")?;
    let data_end           = data_end         .ok_or("Couldn't find end of .data section")?;
     
    let text_flags    = text_flags  .ok_or("Couldn't find .text section flags")?;
    let rodata_flags  = rodata_flags.ok_or("Couldn't find .rodata section flags")?;
    let data_flags    = data_flags  .ok_or("Couldn't find .data section flags")?;

    let text = SectionMemoryBounds {
        start: text_start,
        end: text_end,
        flags: text_flags,
    };
    let rodata = SectionMemoryBounds {
        start: rodata_start,
        end: rodata_end,
        flags: rodata_flags,
    };
    let data = SectionMemoryBounds {
        start: data_start,
        end: data_end,
        flags: data_flags,
    };

    let aggregated_sections_memory_bounds = AggregatedSectionMemoryBounds {
        text,
        rodata,
        data,
    };
    Ok((aggregated_sections_memory_bounds, sections_memory_bounds))
}


/// Gets the physical memory occupied by vga.
/// 
/// Returns (start_physical_address, size, entryflags). 
pub fn get_vga_mem_addr(
) -> Result<(PhysicalAddress, usize, EntryFlags), &'static str> {
    const VGA_DISPLAY_PHYS_START: usize = 0xA_0000;
    const VGA_DISPLAY_PHYS_END: usize = 0xC_0000;
    let vga_size_in_bytes: usize = VGA_DISPLAY_PHYS_END - VGA_DISPLAY_PHYS_START;
    let vga_display_flags =
        EntryFlags::PRESENT | EntryFlags::WRITABLE | EntryFlags::GLOBAL | EntryFlags::NO_CACHE;

    Ok((
        PhysicalAddress::new(VGA_DISPLAY_PHYS_START).ok_or("invalid VGA starting physical address")?,
        vga_size_in_bytes,
        vga_display_flags,
    ))
}

/// Flushes the specific virtual address in TLB. 
pub fn tlb_flush_virt_addr(vaddr: VirtualAddress) {
    tlb::flush(x86_64::VirtAddr::new(vaddr.value() as u64));
}

/// Flushes the whole TLB. 
pub fn tlb_flush_all() {
    tlb::flush_all();
}

/// Returns the current top-level page table address.
pub fn get_p4() -> PhysicalAddress {
    PhysicalAddress::new_canonical(
        Cr3::read_raw().0.start_address().as_u64() as usize
    )
}
