//! This crate implements the virtual memory subsystem interfaces for Theseus on x86_64.
//! `memory_interface` uses this crate to manipulate the memory subsystem on x86_64;

#![no_std]
#![feature(asm)]
#![feature(ptr_internals)]
#![feature(core_intrinsics)]
#![feature(unboxed_closures)]
#![feature(step_trait, range_is_empty)]

extern crate alloc;
extern crate multiboot2;
#[macro_use]
extern crate log;
extern crate irq_safety;
extern crate kernel_config;
extern crate memory_address;
extern crate page_table_x86;
pub extern crate x86_64;

/// Export arch-specific information structure to `memory`.
pub use multiboot2::BootInformation;
/// This is top arch-specific memory crate. Export all memory-related definitions here.
pub use page_table_x86::{EntryFlags, get_p4_address, set_new_p4};
pub use x86_64::{instructions::tlb};

use alloc::sync::Arc;
use alloc::vec::Vec;
use core::ops::DerefMut;
use irq_safety::MutexIrqSafe;
use kernel_config::memory::KERNEL_OFFSET;
use memory_address::{Frame, PhysicalAddress, PhysicalMemoryArea, VirtualAddress, VirtualMemoryArea};
use multiboot2::MemoryMapTag;

/// Get the address of memory occupied by the loaded kernel.
/// Returns the following tuple, if successful:
///
///  * The kernel's start physical address,
///  * the kernel's end physical address,
///  * the kernels' end virtual address
pub fn get_kernel_address(
    boot_info: &BootInformation,
) -> Result<
    (
        PhysicalAddress,
        PhysicalAddress,
        VirtualAddress,
        &'static MemoryMapTag,
    ),
    &'static str,
> {
    let memory_map_tag = boot_info
        .memory_map_tag()
        .ok_or("Memory map tag not found")?;
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

    Ok((
        kernel_phys_start,
        kernel_phys_end,
        kernel_virt_end,
        memory_map_tag,
    ))
}

/// Get the memory areas occupied by the loaded kernel. Parse the list of physical memory areas from multiboot.
/// Returns the following tuple, if successful:
///
///  * A list of avaiable physical memory areas,
///  * the number of occupied areas, i.e. the index of the next ,
///  * the kernels' end virtual address
pub fn get_available_memory(
    memory_map_tag: &'static MemoryMapTag,
    kernel_phys_end: PhysicalAddress,
) -> Result<([PhysicalMemoryArea; 32], usize), &'static str> {
    let mut available: [PhysicalMemoryArea; 32] = Default::default();
    let mut avail_index = 0;
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

        available[avail_index] = PhysicalMemoryArea {
            base_addr: start_paddr,
            size_in_bytes: area_size,
            typ: 1,
            acpi: 0,
        };

        info!(
            "--> memory region established: start={:#x}, size_in_bytes={:#x}",
            available[avail_index].base_addr, available[avail_index].size_in_bytes
        );
        // print_early!("--> memory region established: start={:#x}, size_in_bytes={:#x}\n", available[avail_index].base_addr, available[avail_index].size_in_bytes);
        avail_index += 1;
    }

    Ok((available, avail_index))
}

/// Calculate the bounds of physical memory that is occupied by modules we've loaded.
/// Returns (start_address, end_address).
pub fn get_modules_address(boot_info: &BootInformation) -> (usize, usize) {
    let mut mod_min = usize::max_value();
    let mut mod_max = 0;
    use core::cmp::{max, min};

    for m in boot_info.module_tags() {
        mod_min = min(mod_min, m.start_address() as usize);
        mod_max = max(mod_max, m.end_address() as usize);
    }
    (mod_min, mod_max)
}

/// Get the physical memory area occupied by the multiboot information
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

pub fn get_boot_info_address(
    boot_info: &BootInformation,
) -> Result<(VirtualAddress, VirtualAddress), &'static str> {
    let boot_info_start_vaddr = VirtualAddress::new(boot_info.start_address())?;
    let boot_info_end_vaddr = VirtualAddress::new(boot_info.end_address())?;
    Ok((boot_info_start_vaddr, boot_info_end_vaddr))
}

pub fn add_section_vmem_areas(
    boot_info: &BootInformation,
    vmas: &mut [VirtualMemoryArea; 32],
) -> Result<(
        usize,
        Option<(VirtualAddress, PhysicalAddress)>,
        Option<(VirtualAddress, PhysicalAddress)>,
        Option<(VirtualAddress, PhysicalAddress)>,
        Option<(VirtualAddress, PhysicalAddress)>,
        Option<(VirtualAddress, PhysicalAddress)>,
        Option<(VirtualAddress, PhysicalAddress)>,
        Option<EntryFlags>,
        Option<EntryFlags>,
        Option<EntryFlags>,
        Vec<(PhysicalAddress, VirtualAddress, usize, EntryFlags)>,
    ),
    &'static str,
> {
    let elf_sections_tag = try!(boot_info.elf_sections_tag().ok_or("no Elf sections tag present!"));   

    let mut index = 0;
    let mut text_start:   Option<(VirtualAddress, PhysicalAddress)> = None;
    let mut text_end:     Option<(VirtualAddress, PhysicalAddress)> = None;
    let mut rodata_start: Option<(VirtualAddress, PhysicalAddress)> = None;
    let mut rodata_end:   Option<(VirtualAddress, PhysicalAddress)> = None;
    let mut data_start:   Option<(VirtualAddress, PhysicalAddress)> = None;
    let mut data_end:     Option<(VirtualAddress, PhysicalAddress)> = None;

    let mut text_flags:       Option<EntryFlags> = None;
    let mut rodata_flags:     Option<EntryFlags> = None;
    let mut data_flags:       Option<EntryFlags> = None;


    let mut identity_sections: Vec<(PhysicalAddress, VirtualAddress, usize, EntryFlags)> =
        Vec::new();

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

        debug!(
            "Looking at loaded section {} at {:#X}, size {:#X}",
            section.name(),
            section.start_address(),
            section.size()
        );

        if PhysicalAddress::new_canonical(section.start_address() as usize).frame_offset() != 0 {
            error!(
                "Section {} at {:#X}, size {:#X} was not page-aligned!",
                section.name(),
                section.start_address(),
                section.size()
            );
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
                rodata_end = Some((end_virt_addr, end_phys_addr));
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
            _ => {
                error!("Section {} at {:#X}, size {:#X} was not an expected section (.init, .text, .data, .bss, .rodata)", 
                        section.name(), section.start_address(), section.size());
                return Err("Kernel ELF Section had an unexpected name (expected .init, .text, .data, .bss, .rodata)");
            }
        };
        vmas[index] = VirtualMemoryArea::new(
            start_virt_addr,
            section.size() as usize,
            flags,
            static_str_name,
        );
        debug!(
            "     mapping kernel section: {} at addr: {:?}",
            section.name(),
            vmas[index]
        );

        identity_sections.push((
            start_phys_addr,
            start_virt_addr,
            section.size() as usize,
            flags,
        ));

        index += 1;
    } // end of section iterator

    Ok((index, text_start, text_end, rodata_start, rodata_end, data_start, data_end, text_flags, data_flags, rodata_flags, identity_sections))
}
