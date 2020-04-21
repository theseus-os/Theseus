//! This crate implements the virtual memory subsystem interfaces for Theseus on x86_64.
//! `memory` uses this crate to get the memory layout and do other arch-specific operations on x86_64.  
//! 
//! This is the top-level arch-specific memory crate. 
//! All arch-specific definitions for memory system are exported from this crate.

#![no_std]
#![feature(asm)]
#![feature(ptr_internals)]
#![feature(unboxed_closures)]
#![feature(step_trait, range_is_empty)]

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
    Frame, PhysicalAddress, PhysicalMemoryArea, VirtualAddress, SectionMemoryBounds, AggregatedSectionMemoryBounds,
};
use x86_64::{registers::control_regs, instructions::tlb};


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
    )?;
    let kernel_virt_end = VirtualAddress::new(
        elf_sections_tag
            .sections()
            .filter(|s| s.is_allocated())
            .map(|s| s.end_address())
            .max()
            .ok_or("Couldn't find kernel end (virt) address")? as usize,
    )?;
    let kernel_phys_end = PhysicalAddress::new(kernel_virt_end.value() - KERNEL_OFFSET)?;

    Ok((kernel_phys_start, kernel_phys_end, kernel_virt_end))
}

/// Gets the available physical memory areas from the bootloader-provided list.
///
/// Returns the following tuple, if successful:
///  * An array of available physical memory areas,
///  * The number of valid entries in that array.
pub fn get_available_memory(
    boot_info: &BootInformation,
    kernel_phys_end: PhysicalAddress,
) -> Result<([PhysicalMemoryArea; 32], usize), &'static str> {
    // parse the list of physical memory areas from multiboot
    let mut available: [PhysicalMemoryArea; 32] = Default::default();
    let mut avail_index = 0;
    let memory_map_tag = boot_info
        .memory_map_tag()
        .ok_or("Memory map tag not found")?;
    for area in memory_map_tag.memory_areas() {
        let area_start = PhysicalAddress::new(area.start_address() as usize)?;
        let area_end = PhysicalAddress::new(area.end_address() as usize)?;
        let area_size = area.size() as usize;
        debug!(
            "memory area base_addr={:#x} length={:#x} ({:?})",
            area_start, area_size, area
        );

        // optimization: we reserve memory from areas below the end of the kernel's physical address,
        // which includes addresses beneath 1 MB
        if area_end < kernel_phys_end {
            debug!("--> skipping region before kernel_phys_end");
            continue;
        }
        let start_paddr: PhysicalAddress = if area_start >= kernel_phys_end {
            area_start
        } else {
            kernel_phys_end
        };
        let start_paddr = (Frame::containing_address(start_paddr) + 1).start_address(); // align up to next page

        let new_entry = available.get_mut(avail_index).ok_or("Found more than 32 physical memory areas, only 32 are supported.")?;
        *new_entry = PhysicalMemoryArea {
            base_addr: start_paddr,
            size_in_bytes: area_size,
            typ: 1,
            acpi: 0,
        };

        info!("--> memory region established: start={:#x}, size_in_bytes={:#x}",
            new_entry.base_addr, new_entry.size_in_bytes
        );
        avail_index += 1;
    }

    Ok((available, avail_index))
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
) -> Result<PhysicalMemoryArea, &'static str> {
    Ok(PhysicalMemoryArea::new(
        PhysicalAddress::new(boot_info.start_address() - KERNEL_OFFSET)?,
        boot_info.end_address() - boot_info.start_address(),
        1,
        0,
    ))
}


/// Finds the addresses in memory of the main kernel sections, as specified by the given boot information. 
/// 
/// Returns the following tuple, if successful:
///  * The combined size and address bounds of specifically .text, .rodata, and .data. 
///    Each of the three section bounds is aggregated to cover the bounds and sizes of *all* sections that share the same flags.
///  * The list of individual sections found. 
pub fn find_section_memory_bounds(boot_info: &BootInformation) -> Result<(AggregatedSectionMemoryBounds, [Option<SectionMemoryBounds>; 32]), &'static str> {
    let elf_sections_tag = boot_info.elf_sections_tag().ok_or("no Elf sections tag present!")?;

    let mut index = 0;
    let mut text_start: Option<(VirtualAddress, PhysicalAddress)> = None;
    let mut text_end: Option<(VirtualAddress, PhysicalAddress)> = None;
    let mut rodata_start: Option<(VirtualAddress, PhysicalAddress)> = None;
    let mut rodata_end: Option<(VirtualAddress, PhysicalAddress)> = None;
    let mut data_start: Option<(VirtualAddress, PhysicalAddress)> = None;
    let mut data_end: Option<(VirtualAddress, PhysicalAddress)> = None;

    let mut text_flags: Option<EntryFlags> = None;
    let mut rodata_flags: Option<EntryFlags> = None;
    let mut data_flags: Option<EntryFlags> = None;

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

        let start_phys_addr = PhysicalAddress::new(start_phys_addr)?;
        let start_virt_addr = VirtualAddress::new(start_virt_addr)?;
        let end_virt_addr = start_virt_addr + (section.size() as usize);
        let end_phys_addr = start_phys_addr + (section.size() as usize);

        // Currently, we require that the linker script specify that each section should be page-aligned.
        // This isn't truly necessary, but it simplifies the logic here quite a bit. 
        if start_phys_addr.frame_offset() != 0 {
            error!("Section {} at {:#X}, size {:#X} was not page-aligned!", section.name(), section.start_address(), section.size());
            return Err("Kernel ELF Section was not page-aligned");
        }

        // The linker script (linker_higher_half.ld) defines the following order of sections:
        // (1) .init (start of executable pages)
        // (2) .text (end of executable pages)
        // (3) .rodata (start of read-only pages)
        // (4) .eh_frame
        // (5) .gcc_except_table (end of read-only pages)
        // (6) .data (start of read-write pages)
        // (7) .bss (end of read-write pages)
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
        debug!("     mapping kernel section {:?} as {:?} at vaddr: {:#X}, size {:#X} bytes", section.name(), static_str_name, start_virt_addr, section.size());

        sections_memory_bounds[index] = Some(SectionMemoryBounds {
            start: (start_virt_addr, start_phys_addr),
            end: (end_virt_addr, end_phys_addr),
            flags: flags,
        });

        index += 1;
    }

    let text_start    = text_start  .ok_or("Couldn't find start of .text section")?;
    let text_end      = text_end    .ok_or("Couldn't find end of .text section")?;
    let rodata_start  = rodata_start.ok_or("Couldn't find start of .rodata section")?;
    let rodata_end    = rodata_end  .ok_or("Couldn't find end of .rodata section")?;
    let data_start    = data_start  .ok_or("Couldn't find start of .data section")?;
    let data_end      = data_end    .ok_or("Couldn't find start of .data section")?;

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
        PhysicalAddress::new(VGA_DISPLAY_PHYS_START)?,
        vga_size_in_bytes,
        vga_display_flags,
    ))
}

/// Flushes the specific virtual address in TLB. 
pub fn tlb_flush_virt_addr(vaddr: VirtualAddress) {
    tlb::flush(x86_64::VirtualAddress(vaddr.value()));
}

/// Flushes the whole TLB. 
pub fn tlb_flush_all() {
    tlb::flush_all();
}

/// Returns the current top-level page table address.
pub fn get_p4() -> PhysicalAddress {
    PhysicalAddress::new_canonical(control_regs::cr3().0 as usize)
}
