//! This crate implements the virtual memory subsystem interfaces for Theseus on x86_64.
//! `memory` uses this crate to get the memory layout and do other arch-specific operations on x86_64.  
//! 
//! This is the top-level arch-specific memory crate. 
//! All arch-specific definitions for memory system are exported from this crate.

#![no_std]
#![feature(ptr_internals)]
#![feature(unboxed_closures)]

extern crate multiboot2;
#[macro_use] extern crate log;
extern crate kernel_config;
extern crate memory_structs;
extern crate entryflags_x86_64;
extern crate x86_64;

pub use multiboot2::BootInformation;
pub use entryflags_x86_64::EntryFlags;

use kernel_config::memory::KERNEL_OFFSET;
use memory_structs::{
    PhysicalAddress, VirtualAddress, SectionMemoryBounds, AggregatedSectionMemoryBounds,
};
use x86_64::{registers::control::Cr3, instructions::tlb};


/// Finds and returns the relevant addresses for the kernel image loaded into memory by the bootloader.
///
/// Returns the following tuple, if successful:
///  * The kernel's starting physical address,
///  * the kernel's ending physical address,
///  * the kernel's ending virtual address.
pub fn get_kernel_address(
    boot_info: &BootInformation,
) -> Result<(PhysicalAddress, PhysicalAddress, VirtualAddress), &'static str> {
    let elf_sections_tag = boot_info
        .elf_sections_tag()
        .ok_or("Elf sections tag not found")?;

    // Our linker script specifies that the kernel will have the .init section starting at 1MB and ending at 1MB + .init size
    // and all other kernel sections will start at (KERNEL_OFFSET + 1MB) and end at (KERNEL_OFFSET + 1MB + size).
    // So, the start of the kernel is its physical address, but the end of it is its virtual address... confusing, I know
    // Thus, kernel_phys_start is the same as kernel_virt_start initially, but we remap them later in paging::init.
    let kernel_phys_start = PhysicalAddress::new(
        elf_sections_tag
            .sections()
            .filter(|s| s.is_allocated())
            .map(|s| s.start_address())
            .min()
            .ok_or("Couldn't find kernel start (phys) address")? as usize,
    ).ok_or("kernel start physical address was invalid")?;
    let kernel_virt_end = VirtualAddress::new(
        elf_sections_tag
            .sections()
            .filter(|s| s.is_allocated())
            .map(|s| s.end_address())
            .max()
            .ok_or("Couldn't find kernel end (virt) address")? as usize,
    ).ok_or("kernel virtual end address was invalid")?;
    let kernel_phys_end = PhysicalAddress::new(kernel_virt_end.value() - KERNEL_OFFSET)
        .ok_or("kernel end physical address was invalid")?;

    Ok((kernel_phys_start, kernel_phys_end, kernel_virt_end))
}


/// Gets the address bounds of physical memory occupied by all bootloader-loaded modules.
/// 
/// Returns (start_address, end_address).
pub fn get_modules_address(boot_info: &BootInformation) -> (PhysicalAddress, PhysicalAddress) {
    let mut mod_min = usize::max_value();
    let mut mod_max = 0;
    use core::cmp::{max, min};

    for m in boot_info.module_tags() {
        mod_min = min(mod_min, m.start_address() as usize);
        mod_max = max(mod_max, m.end_address() as usize);
    }
    (PhysicalAddress::new_canonical(mod_min), PhysicalAddress::new_canonical(mod_max))
}

/// Gets the physical memory area occupied by the bootloader information.
pub fn get_boot_info_mem_area(
    boot_info: &BootInformation,
) -> Result<(PhysicalAddress, PhysicalAddress), &'static str> {
    Ok((
        PhysicalAddress::new(boot_info.start_address() - KERNEL_OFFSET)
            .ok_or("boot info start physical address was invalid")?,
        PhysicalAddress::new(boot_info.end_address() - KERNEL_OFFSET)
            .ok_or("boot info end physical address was invalid")?,
    ))
}


/// Finds the addresses in memory of the main kernel sections, as specified by the given boot information. 
/// 
/// Returns the following tuple, if successful:
///  * The combined size and address bounds of key sections, e.g., .text, .rodata, .data.
///    Each of the these section bounds is aggregated to cover the bounds and sizes of *all* sections 
///    that share the same page table mapping flags and can thus be logically combined.
///  * The list of all individual sections found. 
pub fn find_section_memory_bounds(boot_info: &BootInformation) -> Result<(AggregatedSectionMemoryBounds, [Option<SectionMemoryBounds>; 32]), &'static str> {
    let elf_sections_tag = boot_info.elf_sections_tag().ok_or("no Elf sections tag present!")?;

    let mut index = 0;
    let mut text_start:        Option<(VirtualAddress, PhysicalAddress)> = None;
    let mut text_end:          Option<(VirtualAddress, PhysicalAddress)> = None;
    let mut rodata_start:      Option<(VirtualAddress, PhysicalAddress)> = None;
    let mut rodata_end:        Option<(VirtualAddress, PhysicalAddress)> = None;
    let mut data_start:        Option<(VirtualAddress, PhysicalAddress)> = None;
    let mut data_end:          Option<(VirtualAddress, PhysicalAddress)> = None;
    let mut stack_start:       Option<(VirtualAddress, PhysicalAddress)> = None;
    let mut stack_end:         Option<(VirtualAddress, PhysicalAddress)> = None;
    let mut page_table_start:  Option<(VirtualAddress, PhysicalAddress)> = None;
    let mut page_table_end:    Option<(VirtualAddress, PhysicalAddress)> = None;

    let mut text_flags:        Option<EntryFlags> = None;
    let mut rodata_flags:      Option<EntryFlags> = None;
    let mut data_flags:        Option<EntryFlags> = None;

    let mut sections_memory_bounds: [Option<SectionMemoryBounds>; 32] = Default::default();

    // map the allocated kernel text sections
    for section in elf_sections_tag.sections() {
        // skip sections that don't need to be loaded into memory
        if section.size() == 0
            || !section.is_allocated()
            || section.name().starts_with(".debug")
        {
            continue;
        }

        debug!("Looking at loaded section {} at {:#X}, size {:#X}", section.name(), section.start_address(), section.size());
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

        let start_phys_addr = PhysicalAddress::new(start_phys_addr).ok_or("section had invalid starting physical address")?;
        let start_virt_addr = VirtualAddress::new(start_virt_addr).ok_or("section had invalid ending physical address")?;
        let end_virt_addr = start_virt_addr + (section.size() as usize);
        let end_phys_addr = start_phys_addr + (section.size() as usize);

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
            ".page_table" => {
                page_table_start = Some((start_virt_addr, start_phys_addr));
                page_table_end   = Some((end_virt_addr, end_phys_addr));
                "initial page_table"
            }
            ".stack" => {
                stack_start = Some((start_virt_addr, start_phys_addr));
                stack_end   = Some((end_virt_addr, end_phys_addr));
                "initial stack"
            }
            _ =>  {
                error!("Section {} at {:#X}, size {:#X} was not an expected section", 
                        section.name(), section.start_address(), section.size());
                return Err("Kernel ELF Section had an unexpected name");
            }
        };
        debug!("     will map kernel section {:?} as {:?} at vaddr: {:#X}, size {:#X} bytes", section.name(), static_str_name, start_virt_addr, section.size());

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
    let data_end           = data_end         .ok_or("Couldn't find start of .data section")?;
    let page_table_start   = page_table_start .ok_or("Couldn't find start of .page_table section")?;
    let page_table_end     = page_table_end   .ok_or("Couldn't find start of .page_table section")?;
    let stack_start        = stack_start      .ok_or("Couldn't find start of .stack section")?;
    let stack_end          = stack_end        .ok_or("Couldn't find start of .stack section")?;
     
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
    let page_table = SectionMemoryBounds {
        start: page_table_start,
        end: page_table_end,
        flags: data_flags, // same flags as data sections
    };
    let stack = SectionMemoryBounds {
        start: stack_start,
        end: stack_end,
        flags: data_flags, // same flags as data sections
    };

    let aggregated_sections_memory_bounds = AggregatedSectionMemoryBounds {
        text,
        rodata,
        data,
        page_table,
        stack,
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
