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

use cortex_a::asm::barrier;
use cortex_a::registers::*;
use tock_registers::interfaces::Writeable;
use tock_registers::interfaces::ReadWriteable;

use memory_structs::{PhysicalAddress, VirtualAddress};

use core::arch::asm;

/// Flushes the specific virtual address in TLB.
///
/// TLBI => tlb invalidate instruction
/// "va" => all translations at execution level
///         using the supplied address
/// "e1" => execution level
pub fn tlb_flush_virt_addr(vaddr: VirtualAddress) {
    unsafe { asm!("tlbi vae1, {}", in(reg) vaddr.value()) };
}

/// Flushes the whole TLB.
///
/// TLBI => tlb invalidate instruction
/// "all" => all translations at execution level
/// "e1" => execution level
pub fn tlb_flush_all() {
    unsafe { asm!("tlbi alle1") };
}

/// Returns the current top-level page table address.
///
/// We use TTBR0 in Theseus to store the
/// top-level page table, so this function
/// reads that register.
pub fn get_p4() -> PhysicalAddress {
    PhysicalAddress::new_canonical(
        TTBR0_EL1.get_baddr() as usize
    )
}

/// Configures paging for Theseus.
///
///
pub fn set_page_table_up(page_table: PhysicalAddress) {
    unsafe {
        // The MAIR register holds up to 8 memory profiles;
        // each profile describes cacheability of the memory.
        // In Theseus, we currently use two profiles: one for
        // device memory (non-cacheable) and one for normal
        // memory (the usual RAM memory, cacheable).
        //
        // For more information on MAIR, See section D17.2.97
        // of [DDI0487l.a](https://l0.pm/arm-ddi0487l.a.pdf).
        MAIR_EL1.write(
            // Attribute 1 - Cacheable normal DRAM.
            MAIR_EL1::Attr1_Normal_Outer::WriteBack_NonTransient_ReadWriteAlloc +
            MAIR_EL1::Attr1_Normal_Inner::WriteBack_NonTransient_ReadWriteAlloc +

            // Attribute 0 - Device.
            MAIR_EL1::Attr0_Device::nonGathering_nonReordering_EarlyWriteAck,
        );

        // The translation control register contains most of
        // parameters for address translation; these parameters
        // notably define the format for page table entry flags.
        TCR_EL1.write(
            // TCR_EL1::DS is not exposed by `cortex_a`.
            // by default (cleared) it means we cannot
            // use 52-bit output-address mode.

            // Whether to use the top-byte of virtual
            // addresses for tagged addresses ("Ignored")
            // or to use them in the page table walk ("Used").
            // With our four-level paging, however, the top-byte
            // is not used for page table walks anyway.
            //   TCR_EL1::TBI0::Used
            // | TCR_EL1::TBI1::Used

            // Translation Granule Size = Page Size
            // => four kilobytes
              TCR_EL1::TG0::KiB_4
            // + TCR_EL1::TG1::KiB_4

            // These fields could only be used if we had access
            // to the DS field, and if we set it to one. Indeed,
            // when DS=1, the shareability fields of page
            // descriptors are replaced by some bits of the output
            // address; the shareability is constant for the whole
            // page table: one for TTBR0 and one for TTBR1.
            // + TCR_EL1::SH0::Inner
            // + TCR_EL1::SH1::Inner

            // ASID Size. The upper 8 bits of TTBR0_EL1 and
            // TTBR1_EL1 are ignored by hardware for every
            // purpose except reading back the register, and
            // are treated as if they are all zeros for when
            // used for allocation and matching entries in the TLB.
            + TCR_EL1::AS::ASID8Bits

            // We currently output 48 bits of physical
            // address on aarch64.
            + TCR_EL1::IPS::Bits_48

            // Translation table walk disable. This bit controls
            // whether a translation table walk is performed on
            // a TLB miss.
            + TCR_EL1::EPD0::EnableTTBR0Walks
            // + TCR_EL1::EPD1::EnableTTBR1Walks

            // From which TTBR to read the ASID, when comparing the
            // current address space with the one from an address.
            // Theseus only has one address space so this is irrelevant.
            // + TCR_EL1::A1::TTBR0

            // Controls the size of the memory region addressed
            // by page table walks. We have to write 64 - (max
            // number of bits in a region address): 64 - 48 = 16.
            + TCR_EL1::T0SZ.val(16)
            // + TCR_EL1::T1SZ.val(16)

            // I don't understand when these flags are used at the moment
            // + TCR_EL1::ORGN0::WriteBack_ReadAlloc_WriteAlloc_Cacheable
            // + TCR_EL1::ORGN1::NonCacheable
            // + TCR_EL1::IRGN0::WriteBack_ReadAlloc_WriteAlloc_Cacheable
            // + TCR_EL1::IRGN1::NonCacheable

            // Allow the MMU to update the ACCESSED flag.
            + TCR_EL1::HA::Enable

            // Allow the MMU to update the DIRTY flag.
            + TCR_EL1::HD::Enable

        );

        TTBR0_EL1.set_baddr(page_table.value() as u64);
        // TTBR1_EL1.set_baddr(page_table.value() as u64);

        barrier::isb(barrier::SY);
    }
}
