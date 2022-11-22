//! This crate implements the virtual memory subsystem interfaces for Theseus on aarch64.
//! `memory` uses this crate to get the memory layout and do other arch-specific operations on aarch64.  
//! 
//! This is the top-level arch-specific memory crate. 
//! All arch-specific definitions for memory system are exported from this crate.

#![no_std]
#![feature(ptr_internals)]
#![feature(unboxed_closures)]

extern crate memory_structs;
extern crate cortex_a;
extern crate tock_registers;

use cortex_a::registers::*;

use memory_structs::{PhysicalAddress, VirtualAddress};

use core::arch::asm;

/// Flushes the specific virtual address in TLB.
pub fn tlb_flush_virt_addr(_vaddr: VirtualAddress) {
    tlb_flush_all()
}

/// Flushes the whole TLB.
pub fn tlb_flush_all() {
    unsafe { asm!("tlbi alle1") };
}

/// Returns the current top-level page table address.
pub fn get_p4() -> PhysicalAddress {
    PhysicalAddress::new_canonical(
        TTBR0_EL1.get_baddr() as usize
    )
}
