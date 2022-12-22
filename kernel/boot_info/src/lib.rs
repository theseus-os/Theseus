//! This crate provides an abstraction over multiboot2 and UEFI boot
//! information.
//!
//! It allows the kernel's initialisation to be the same, regardless of whether
//! it was booted using BIOS or UEFI.

#![feature(type_alias_impl_trait)]
#![no_std]

#[cfg(feature = "multiboot2")]
pub mod multiboot2;
#[cfg(feature = "uefi")]
pub mod uefi;

use core::{iter::Iterator, ops::Range};
use memory_structs::{PhysicalAddress, VirtualAddress};

pub trait MemoryRegion {
    /// Returns the region's starting physical address.
    fn start(&self) -> PhysicalAddress;

    /// Returns the region's length.
    fn len(&self) -> usize;

    /// Returns whether the region can be used by the frame allocator.
    fn is_usable(&self) -> bool;
}

pub trait ElfSection {
    /// Returns the section's name.
    fn name(&self) -> &str;

    /// Returns the section's starting virtual address.
    fn start(&self) -> VirtualAddress;

    /// Returns the section's length.
    fn len(&self) -> usize;

    /// Returns the section's flags.
    fn flags(&self) -> ElfSectionFlags;
}

bitflags::bitflags! {
    /// ELF section flags.
    pub struct ElfSectionFlags: u64 {
        /// The section contains data that should be writable during program execution.
        const WRITABLE = 0x1;

        /// The section occupies memory during the process execution.
        const ALLOCATED = 0x2;

        /// The section contains executable machine instructions.
        const EXECUTABLE = 0x4;
    }
}

pub trait Module {
    /// Returns the module's name.
    fn name(&self) -> Result<&str, &'static str>;

    /// Returns the module's starting physical address.
    fn start(&self) -> PhysicalAddress;

    /// Returns the module's length.
    fn len(&self) -> usize;
}

pub trait BootInformation: 'static {
    type MemoryRegion<'a>: MemoryRegion;
    type MemoryRegions<'a>: Iterator<Item = Self::MemoryRegion<'a>>;

    type ElfSection<'a>: ElfSection;
    type ElfSections<'a>: Iterator<Item = Self::ElfSection<'a>>;

    type Module<'a>: Module;
    type Modules<'a>: Iterator<Item = Self::Module<'a>>;

    /// Returns the boot information's starting virtual address.
    fn start(&self) -> Option<VirtualAddress>;
    /// Returns the boot information's length.
    fn len(&self) -> usize;

    /// Returns the range of physical addresses at which the kernel code is
    /// located.
    fn kernel_memory_range(&self) -> Result<Range<PhysicalAddress>, &'static str>;
    /// Returns the range of physical addresses at which the bootloader
    /// information is located.
    fn bootloader_info_memory_range(&self) -> Result<Range<PhysicalAddress>, &'static str>;
    /// Returns the range of physical addresses at which the modules are
    /// located.
    fn modules_memory_range(&self) -> Result<Range<PhysicalAddress>, &'static str>;

    /// Returns memory regions describing the physical memory.
    fn memory_regions(&self) -> Result<Self::MemoryRegions<'_>, &'static str>;
    /// Returns the kernel's ELF sections.
    fn elf_sections(&self) -> Result<Self::ElfSections<'_>, &'static str>;
    /// Returns the modules found in the kernel image.
    fn modules(&self) -> Self::Modules<'_>;

    /// Returns the RSDP if it was provided by the bootloader.
    fn rsdp(&self) -> Option<PhysicalAddress>;

    /// Returns the stack size in bytes.
    fn stack_size(&self) -> Result<usize, &'static str>;
}
