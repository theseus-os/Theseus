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
extern crate x86_64;

/// Export arch-specific information structure to `memory`.
pub use multiboot2::BootInformation;
/// This is top arch-specific memory crate. Export all memory-related definitions here.
pub use page_table_x86::*;

use alloc::sync::Arc;
use alloc::vec::Vec;
use core::ops::DerefMut;
use irq_safety::MutexIrqSafe;
use kernel_config::memory::KERNEL_OFFSET;
use multiboot2::MemoryMapTag;
use memory_address::{PhysicalMemoryArea, Frame};

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
        memory_address::PhysicalAddress,
        memory_address::PhysicalAddress,
        memory_address::VirtualAddress,
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
    let kernel_phys_start = memory_address::PhysicalAddress::new(
        elf_sections_tag
            .sections()
            .filter(|s| s.is_allocated())
            .map(|s| s.start_address())
            .min()
            .ok_or("Couldn't find kernel start (phys) address")? as usize,
    )?;
    let kernel_virt_end = memory_address::VirtualAddress::new(
        elf_sections_tag
            .sections()
            .filter(|s| s.is_allocated())
            .map(|s| s.end_address())
            .max()
            .ok_or("Couldn't find kernel end (virt) address")? as usize,
    )?;
    let kernel_phys_end =
        memory_address::PhysicalAddress::new(kernel_virt_end.value() - KERNEL_OFFSET)?;

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
    kernel_phys_end: memory_address::PhysicalAddress,
) -> Result<([PhysicalMemoryArea; 32], usize), &'static str> {
    let mut available: [PhysicalMemoryArea; 32] = Default::default();
    let mut avail_index = 0;
    for area in memory_map_tag.memory_areas() {
        let area_start = memory_address::PhysicalAddress::new(area.start_address() as usize)?;
        let area_end = memory_address::PhysicalAddress::new(area.end_address() as usize)?;
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
        let start_paddr: memory_address::PhysicalAddress = if area_start >= kernel_phys_end {
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

/// calculate the bounds of physical memory that is occupied by modules we've loaded 
/// (we can reclaim this later after the module is loaded, but not until then)
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