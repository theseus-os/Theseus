use crate::{ElfSection, MemoryArea, MemoryAreaType, PhysicalAddressRange};
use core::{cmp, iter::Iterator};
use kernel_config::memory::KERNEL_OFFSET;
use memory_structs::{PhysicalAddress, VirtualAddress};

impl<'a> MemoryArea for &'a multiboot2::MemoryArea {
    fn start(&self) -> usize {
        self.start_address() as usize
    }

    fn end(&self) -> usize {
        self.end_address() as usize
    }

    fn ty(&self) -> MemoryAreaType {
        match self.typ() {
            multiboot2::MemoryAreaType::Available => MemoryAreaType::Available,
            // FIXME
            _ => MemoryAreaType::Reserved,
        }
    }
}

impl ElfSection for multiboot2::ElfSection {
    fn name(&self) -> &str {
        self.name()
    }

    fn is_allocated(&self) -> bool {
        self.is_allocated()
    }

    fn start(&self) -> usize {
        self.start_address() as usize
    }

    fn size(&self) -> usize {
        self.size() as usize
    }
}

type MemoryAreaIterator<'a> = impl Iterator<Item = &'a multiboot2::MemoryArea>;

pub struct MemoryAreas<'a> {
    inner: MemoryAreaIterator<'a>,
}

impl<'a> Iterator for MemoryAreas<'a> {
    type Item = &'a multiboot2::MemoryArea;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next()
    }
}

impl crate::BootInformation for multiboot2::BootInformation {
    type MemoryArea<'a> = &'a multiboot2::MemoryArea;
    type MemoryAreas<'a> = MemoryAreas<'a>;

    type ElfSection<'a> = multiboot2::ElfSection;
    type ElfSections<'a> = multiboot2::ElfSectionIter;

    fn size(&self) -> usize {
        self.total_size()
    }

    fn kernel_mapping(&self) -> Result<PhysicalAddressRange, &'static str> {
        // Our linker script specifies that the kernel will have the .init section
        // starting at 1MB and ending at 1MB + .init size and all other kernel sections
        // will start at (KERNEL_OFFSET + 1MB) and end at (KERNEL_OFFSET + 1MB + size).
        // So, the start of the kernel is its physical address, but the end of it is its
        // virtual address... confusing, I know. Thus, kernel_phys_start is the same as
        // kernel_virt_start initially, but we remap them later in paging::init.
        let start = PhysicalAddress::new(
            self.elf_sections()?
                .into_iter()
                .filter(|s| s.is_allocated())
                .map(|s| s.start_address())
                .min()
                .ok_or("couldn't find kernel start address")? as usize,
        )
        .ok_or("kernel physical start address was invalid")?;

        let virtual_end = VirtualAddress::new(
            self.elf_sections()?
                .into_iter()
                .filter(|s| s.is_allocated())
                .map(|s| s.end_address())
                .max()
                .ok_or("couldn't find kernel end address")? as usize,
        )
        .ok_or("kernel virtual end address was invalid")?;
        let physical_end = PhysicalAddress::new(virtual_end.value() - KERNEL_OFFSET)
            .ok_or("kernel physical end address was invalid")?;

        Ok(PhysicalAddressRange {
            start,
            end: physical_end,
        })
    }

    fn bootloader_info_mapping(&self) -> Result<PhysicalAddressRange, &'static str> {
        Ok(PhysicalAddressRange {
            start: PhysicalAddress::new(self.start_address() - KERNEL_OFFSET)
                .ok_or("invalid bootloader info start address")?,
            end: PhysicalAddress::new(self.end_address() - KERNEL_OFFSET)
                .ok_or("invalid bootloader info end address")?,
        })
    }

    fn modules_mapping(&self) -> Result<PhysicalAddressRange, &'static str> {
        let mut min = usize::MAX;
        let mut max = 0;

        for module in self.module_tags() {
            min = cmp::min(min, module.start_address() as usize);
            max = cmp::max(max, module.end_address() as usize);
        }

        Ok(PhysicalAddressRange {
            start: PhysicalAddress::new_canonical(min),
            end: PhysicalAddress::new_canonical(max),
        })
    }

    fn memory_areas(&self) -> Result<Self::MemoryAreas<'_>, &'static str> {
        Ok(MemoryAreas {
            inner: self
                .memory_map_tag()
                .ok_or("no memory map tag")?
                .memory_areas(),
        })
    }

    fn elf_sections(&self) -> Result<Self::ElfSections<'static>, &'static str> {
        Ok(self
            .elf_sections_tag()
            .ok_or("no elf sections tag")?
            .sections())
    }
}
