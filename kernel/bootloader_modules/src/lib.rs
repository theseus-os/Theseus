//! An abstraction for bootloader-provided "modules".

#![no_std]

extern crate alloc;
extern crate memory_structs;

use alloc::string::String;
use memory_structs::PhysicalAddress;

/// A record of a bootloader module's name and location in physical memory.
#[derive(Debug)]
pub struct BootloaderModule {
    /// The starting address of this module, inclusive.
    start_paddr: PhysicalAddress,
    /// The ending address of this module, exclusive.
    end_paddr: PhysicalAddress,
    /// The name of this module, i.e.,
    /// the filename it was given in the bootloader's cfg file.
    name: String,
}
impl BootloaderModule {
    pub fn new(
        start_paddr: PhysicalAddress,
        end_paddr: PhysicalAddress,
        name: String
    ) -> BootloaderModule {
        BootloaderModule { start_paddr, end_paddr, name }
    } 

    pub fn start_address(&self) -> PhysicalAddress {
        self.start_paddr
    }

    pub fn end_address(&self) -> PhysicalAddress {
        self.end_paddr
    }

    pub fn name(&self) -> &String {
        &self.name
    }

    pub fn size_in_bytes(&self) -> usize {
        self.end_paddr.value() - self.start_paddr.value()
    }
}
