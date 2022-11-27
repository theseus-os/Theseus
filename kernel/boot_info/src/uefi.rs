use core::iter::Iterator;
use memory_structs::PhysicalAddress;

pub struct MemoryArea;

impl crate::MemoryArea for MemoryArea {
    fn start(&self) -> usize {
        todo!()
    }

    fn size(&self) -> usize {
        todo!()
    }

    fn ty(&self) -> crate::MemoryAreaType {
        todo!()
    }
}

pub struct MemoryAreas;

impl Iterator for MemoryAreas {
    type Item = MemoryArea;

    fn next(&mut self) -> Option<Self::Item> {
        todo!()
    }
}

pub struct ElfSection;

impl crate::ElfSection for ElfSection {
    fn name(&self) -> &str {
        todo!()
    }

    fn is_allocated(&self) -> bool {
        todo!()
    }

    fn start(&self) -> usize {
        todo!()
    }

    fn size(&self) -> usize {
        todo!()
    }

    fn flags(&self) -> crate::ElfSectionFlags {
        todo!()
    }
}

pub struct ElfSections;

impl Iterator for ElfSections {
    type Item = ElfSection;

    fn next(&mut self) -> Option<Self::Item> {
        todo!()
    }
}

pub struct Module;

impl crate::Module for Module {
    fn name(&self) -> Result<&str, &'static str> {
        todo!()
    }

    fn start(&self) -> usize {
        todo!()
    }

    fn end(&self) -> usize {
        todo!()
    }
}

pub struct Modules;

impl Iterator for Modules {
    type Item = Module;

    fn next(&mut self) -> Option<Self::Item> {
        todo!()
    }
}

impl crate::BootInformation for &'static bootloader_api::BootInfo {
    type MemoryArea<'a> = MemoryArea;

    type MemoryAreas<'a> = MemoryAreas;

    type ElfSection<'a> = ElfSection;

    type ElfSections<'a> = ElfSections;

    type Module<'a> = Module;

    type Modules<'a> = Modules;

    fn size(&self) -> usize {
        todo!()
    }

    // FIXME: Is using physical addresses ok?

    fn kernel_memory_range(&self) -> Result<core::ops::Range<PhysicalAddress>, &'static str> {
        todo!()
    }

    fn bootloader_info_memory_range(
        &self,
    ) -> Result<core::ops::Range<PhysicalAddress>, &'static str> {
        todo!()
    }

    fn modules_memory_range(&self) -> Result<core::ops::Range<PhysicalAddress>, &'static str> {
        todo!()
    }

    fn memory_areas(&self) -> Result<Self::MemoryAreas<'_>, &'static str> {
        todo!()
    }

    fn elf_sections(&self) -> Result<Self::ElfSections<'_>, &'static str> {
        todo!()
    }

    fn modules(&self) -> Self::Modules<'_> {
        todo!()
    }
}
