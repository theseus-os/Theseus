#![feature(type_alias_impl_trait)]
#![no_std]

// FIXME: usize casts

#[cfg(feature = "multiboot2")]
pub mod multiboot2;
#[cfg(feature = "uefi")]
pub mod uefi;

use core::{iter::Iterator, ops::Range};
use memory_structs::{Frame, Page, PhysicalAddress, VirtualAddress};

pub trait MemoryArea {
    fn start(&self) -> usize;
    fn size(&self) -> usize;
    fn ty(&self) -> MemoryAreaType;
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum MemoryAreaType {
    Available,
    Reserved,
}

pub trait ElfSection {
    fn name(&self) -> &str;
    fn start(&self) -> usize;
    fn size(&self) -> usize;
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
    fn name(&self) -> Result<&str, &'static str>;
    fn start(&self) -> usize;
    fn end(&self) -> usize;
}

pub trait Mapping {
    fn page(&self) -> Page;
    fn frame(&self) -> Frame;
}

pub trait BootInformation: 'static {
    type MemoryArea<'a>: MemoryArea;
    type MemoryAreas<'a>: Iterator<Item = Self::MemoryArea<'a>>;

    type ElfSection<'a>: ElfSection;
    type ElfSections<'a>: Iterator<Item = Self::ElfSection<'a>>;

    type Module<'a>: Module;
    type Modules<'a>: Iterator<Item = Self::Module<'a>>;

    /// Returns the start of the boot information struct.
    fn start(&self) -> VirtualAddress;
    /// Returns the size of the boot information struct.
    fn size(&self) -> usize;

    fn kernel_memory_range(&self) -> Result<Range<PhysicalAddress>, &'static str>;
    fn bootloader_info_memory_range(&self) -> Result<Range<PhysicalAddress>, &'static str>;
    fn modules_memory_range(&self) -> Result<Range<PhysicalAddress>, &'static str>;
    fn stack_memory_range(&self) -> Range<VirtualAddress>;

    fn memory_areas(&self) -> Result<Self::MemoryAreas<'_>, &'static str>;
    fn elf_sections(&self) -> Result<Self::ElfSections<'_>, &'static str>;
    fn modules(&self) -> Self::Modules<'_>;

    fn rsdp(&self) -> Option<usize>;
}
