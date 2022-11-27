use bootloader_api::info;
use core::iter::Iterator;
use memory_structs::PhysicalAddress;
use xmas_elf::ElfFile;

use crate::{ElfSectionFlags, MemoryAreaType};

pub struct MemoryArea;

impl<'a> crate::MemoryArea for &'a info::MemoryRegion {
    fn start(&self) -> usize {
        self.start as usize
    }

    fn size(&self) -> usize {
        (self.end - self.start) as usize
    }

    fn ty(&self) -> MemoryAreaType {
        match self.kind {
            info::MemoryRegionKind::Usable => MemoryAreaType::Available,
            _ => MemoryAreaType::Reserved,
        }
    }
}

pub struct ElfSection {
    name: Option<&'static str>,
    start: usize,
    size: usize,
    flags: ElfSectionFlags,
}

impl crate::ElfSection for ElfSection {
    fn name(&self) -> &str {
        self.name.unwrap_or_default()
    }

    fn start(&self) -> usize {
        self.start
    }

    fn size(&self) -> usize {
        self.size
    }

    fn flags(&self) -> crate::ElfSectionFlags {
        self.flags
    }
}

pub struct ElfSections {
    file: ElfFile<'static>,
    index: u16,
}

impl Iterator for ElfSections {
    type Item = ElfSection;

    fn next(&mut self) -> Option<Self::Item> {
        let count = self.file.header.pt2.sh_count();
        if self.index >= count {
            return None;
        }

        let section = self.file.section_header(self.index).ok();
        self.index += 1;
        let section = section?;

        // TODO: Ideally we would use a lending iterator rather than populating the
        // fields.

        Some(ElfSection {
            name: section.get_name(&self.file).ok(),
            start: section.address() as usize,
            size: section.size() as usize,
            flags: ElfSectionFlags::from_bits_truncate(section.flags()),
        })
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
    type MemoryArea<'a> = &'a info::MemoryRegion;

    type MemoryAreas<'a> = core::slice::Iter<'a, info::MemoryRegion>;

    type ElfSection<'a> = ElfSection;

    type ElfSections<'a> = ElfSections;

    type Module<'a> = Module;

    type Modules<'a> = Modules;

    fn size(&self) -> usize {
        todo!();
    }

    // FIXME: Is using physical addresses ok?

    // The bootloader creates two memory regions with the bootloader type. The first
    // one always starts at 0x1000 and contains the page table, boot info, etc. The
    // second one starts at some other address and contains the nano_core elf file.
    // It is the same size as the nano_core binary.

    fn kernel_memory_range(&self) -> Result<core::ops::Range<PhysicalAddress>, &'static str> {
        let mut iter = self
            .memory_regions
            .iter()
            .filter(|region| region.kind == info::MemoryRegionKind::Bootloader)
            .filter(|region| region.start != 0x1000);

        let kernel_memory_region = iter.next().ok_or("no kernel memory region")?;

        if iter.next().is_some() {
            Err("multiple potential kernel memory regions")
        } else {
            let start = PhysicalAddress::new(kernel_memory_region.start as usize)
                .ok_or("invalid kernel start address")?;
            let end = PhysicalAddress::new(kernel_memory_region.end as usize)
                .ok_or("invalid kernel end address")?;
            Ok(start..end)
        }
    }

    fn bootloader_info_memory_range(
        &self,
    ) -> Result<core::ops::Range<PhysicalAddress>, &'static str> {
        let mut iter = self
            .memory_regions
            .iter()
            .filter(|region| region.kind == info::MemoryRegionKind::Bootloader)
            .filter(|region| region.start == 0x1000);

        let _bootloader_memory_region = iter.next().ok_or("no bootloader info memory region")?;
        if iter.next().is_some() {
            Err("multiple potential bootloader memory info memory regions")
        } else {
            todo!();
        }
    }

    fn modules_memory_range(&self) -> Result<core::ops::Range<PhysicalAddress>, &'static str> {
        todo!()
    }

    fn memory_areas(&self) -> Result<Self::MemoryAreas<'_>, &'static str> {
        Ok(self.memory_regions.iter())
    }

    // FIXME: This is not static. It must not be used after the new page table is
    // swapped in.
    fn elf_sections(&self) -> Result<Self::ElfSections<'static>, &'static str> {
        let kernel_memory_range = self.kernel_memory_range()?;
        let physical_memory_offset = self
            .physical_memory_offset
            .into_option()
            .ok_or("physical memory offset not given")?;

        let kernel_virtual_start =
            (usize::from(kernel_memory_range.start) + physical_memory_offset as usize) as *const u8;
        let kernel_length =
            usize::from(kernel_memory_range.end) - usize::from(kernel_memory_range.start);

        let kernel_bytes: &'static [u8] =
            unsafe { core::slice::from_raw_parts(kernel_virtual_start, kernel_length) };

        let file = xmas_elf::ElfFile::new(kernel_bytes)?;
        Ok(ElfSections { file, index: 0 })
    }

    fn modules(&self) -> Self::Modules<'_> {
        todo!()
    }
}
