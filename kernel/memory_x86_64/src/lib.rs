//! This crate implements the virtual memory subsystem interfaces for Theseus on x86_64.
//!
//! The `memory` crate uses this crate to obtain the multiboot2-provided memory layout
//! of the base kernel image (nano_core), and to do other arch-specific operations on x86_64.

#![no_std]
#![feature(ptr_internals)]
#![feature(unboxed_closures)]

extern crate memory_structs;
extern crate pte_flags;
extern crate x86_64;
extern crate boot_info;

pub use boot_info::{BootInformation, ElfSection, Module};
use pte_flags::PteFlags;

use memory_structs::{PhysicalAddress, VirtualAddress};
use x86_64::{registers::control::Cr3, instructions::tlb};

/// Gets the physical memory occupied by vga.
/// 
/// Returns (start_physical_address, size, PteFlags). 
pub fn get_vga_mem_addr(
) -> Result<(PhysicalAddress, usize, PteFlags), &'static str> {
    const VGA_DISPLAY_PHYS_START: usize = 0xA_0000;
    const VGA_DISPLAY_PHYS_END: usize = 0xC_0000;
    let vga_size_in_bytes: usize = VGA_DISPLAY_PHYS_END - VGA_DISPLAY_PHYS_START;
    let vga_display_flags = PteFlags::new()
        .valid(true)
        .writable(true)
        .device_memory(true); // TODO: set as write-combining (WC)
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
