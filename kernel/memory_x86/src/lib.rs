//! This crate implements the virtual memory subsystem interfaces for Theseus on x86_64. 
//! `memory_interface` uses this crate to manipulate the memory subsystem on x86_64;

#![no_std]
#![feature(asm)]
#![feature(ptr_internals)]
#![feature(core_intrinsics)]
#![feature(unboxed_closures)]
#![feature(step_trait, range_is_empty)]

extern crate multiboot2;
extern crate alloc;
#[macro_use] extern crate log;
extern crate irq_safety;
extern crate kernel_config;
extern crate x86_64;
extern crate page_table_x86;
extern crate memory_address;

/// Export arch-specific information structure to `memory`. 
pub use multiboot2::BootInformation;
/// This is top arch-specific memory crate. Export all memory-related definitions here.
pub use page_table_x86::*;

use core::ops::DerefMut;
use irq_safety::MutexIrqSafe;
use alloc::vec::Vec;
use alloc::sync::Arc;
use kernel_config::memory::{KERNEL_OFFSET};
use multiboot2::MemoryMapTag;


pub fn get_kernel_address(boot_info: &BootInformation) -> Result<(memory_address::PhysicalAddress, memory_address::PhysicalAddress, memory_address::VirtualAddress, &'static MemoryMapTag), &'static str> {
    let memory_map_tag = boot_info.memory_map_tag().ok_or("Memory map tag not found")?;
    let elf_sections_tag = boot_info.elf_sections_tag().ok_or("Elf sections tag not found")?;

    // Our linker script specifies that the kernel will have the .init section starting at 1MB and ending at 1MB + .init size
    // and all other kernel sections will start at (KERNEL_OFFSET + 1MB) and end at (KERNEL_OFFSET + 1MB + size).
    // So, the start of the kernel is its physical address, but the end of it is its virtual address... confusing, I know
    // Thus, kernel_phys_start is the same as kernel_virt_start initially, but we remap them later in paging::init.
    let kernel_phys_start = memory_address::PhysicalAddress::new(
        elf_sections_tag.sections()
            .filter(|s| s.is_allocated())
            .map(|s| s.start_address())
            .min()
            .ok_or("Couldn't find kernel start (phys) address")? as usize
    )?;
    let kernel_virt_end = memory_address::VirtualAddress::new(
        elf_sections_tag.sections()
            .filter(|s| s.is_allocated())
            .map(|s| s.end_address())
            .max()
            .ok_or("Couldn't find kernel end (virt) address")? as usize
    )?;
    let kernel_phys_end = memory_address::PhysicalAddress::new(kernel_virt_end.value() - KERNEL_OFFSET)?;

    Ok((kernel_phys_start, kernel_phys_end, kernel_virt_end, memory_map_tag))
}

