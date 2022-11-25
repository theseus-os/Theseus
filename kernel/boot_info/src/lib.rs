#![feature(type_alias_impl_trait)]
#![no_std]

// FIXME: usize casts

#[cfg(feature = "multiboot2")]
pub mod multiboot2;
#[cfg(feature = "uefi")]
pub mod uefi;

use core::iter::Iterator;
use memory_structs::PhysicalAddress;

pub struct PhysicalAddressRange {
    pub start: PhysicalAddress,
    pub end: PhysicalAddress,
}

pub trait MemoryArea {
    fn start(&self) -> usize;
    fn end(&self) -> usize;
    fn ty(&self) -> MemoryAreaType;
}

pub enum MemoryAreaType {
    Available,
    Reserved,
}

pub trait ElfSection {
    fn name(&self) -> &str;
    fn is_allocated(&self) -> bool;
    fn start(&self) -> usize;
    fn size(&self) -> usize;
}

pub trait BootInformation: 'static {
    type MemoryArea<'a>: MemoryArea;
    type MemoryAreas<'a>: Iterator<Item = Self::MemoryArea<'a>>;

    type ElfSection<'a>: ElfSection;
    type ElfSections<'a>: Iterator<Item = Self::ElfSection<'a>>;

    fn size(&self) -> usize;
    fn kernel_mapping(&self) -> Result<PhysicalAddressRange, &'static str>;
    fn bootloader_info_mapping(&self) -> Result<PhysicalAddressRange, &'static str>;
    fn modules_mapping(&self) -> Result<PhysicalAddressRange, &'static str>;
    fn memory_areas(&self) -> Result<Self::MemoryAreas<'_>, &'static str>;
    fn elf_sections(&self) -> Result<Self::ElfSections<'_>, &'static str>;
}
