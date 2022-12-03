use crate::{ElfSectionFlags, MemoryAreaType};
use bootloader_api::info;
use core::{
    iter::{Iterator, Peekable},
    ops::Range,
};
use kernel_config::memory::KERNEL_OFFSET;
use memory_structs::{Frame, Page, PhysicalAddress, VirtualAddress};
use xmas_elf::ElfFile;

pub struct MemoryArea {
    start: usize,
    end: usize,
    ty: MemoryAreaType,
}

impl From<info::MemoryRegion> for MemoryArea {
    fn from(info::MemoryRegion { start, end, kind }: info::MemoryRegion) -> Self {
        Self {
            start: start as usize,
            end: end as usize,
            ty: match kind {
                info::MemoryRegionKind::Usable => MemoryAreaType::Available,
                _ => MemoryAreaType::Reserved,
            },
        }
    }
}

impl crate::MemoryArea for MemoryArea {
    fn start(&self) -> usize {
        self.start
    }

    fn size(&self) -> usize {
        self.end - self.start
    }

    fn ty(&self) -> MemoryAreaType {
        self.ty
    }
}

pub struct MemoryAreas {
    inner: Peekable<core::slice::Iter<'static, info::MemoryRegion>>,
}

impl Iterator for MemoryAreas {
    type Item = MemoryArea;

    fn next(&mut self) -> Option<Self::Item> {
        let mut area: MemoryArea = (*self.inner.next()?).into();

        // UEFI often separates contiguous memory into separate memory regions. We
        // consolidate them to minimise the number of entries in the frame allocator's
        // reserved and available lists.
        while let Some(next) = self.inner.next_if(|next| {
            let next = MemoryArea::from(**next);
            area.ty == next.ty && area.end == next.start
        }) {
            area.end = next.end as usize;
        }

        Some(area)
    }
}

impl<'a> crate::ElfSection for &'a info::ElfSection {
    fn name(&self) -> &str {
        let end = self
            .name
            .iter()
            .position(|byte| *byte == 0)
            .expect("no null byte in elf section name");
        core::str::from_utf8(&self.name[..end]).expect("invalid bytes in module name")
    }

    fn start(&self) -> usize {
        self.start
    }

    fn size(&self) -> usize {
        self.size
    }

    fn flags(&self) -> ElfSectionFlags {
        ElfSectionFlags::from_bits_truncate(self.flags)
    }
}

pub struct Module {
    inner: info::Module,
    regions: &'static info::MemoryRegions,
}

impl crate::Module for Module {
    fn name(&self) -> Result<&str, &'static str> {
        let end = self
            .inner
            .name
            .iter()
            .position(|byte| *byte == 0)
            .ok_or("no null byte in module name")?;
        core::str::from_utf8(&self.inner.name[..end]).map_err(|_| "invalid bytes in module name")
    }

    fn start(&self) -> usize {
        self.regions
            .iter()
            .filter(|region| region.kind == info::MemoryRegionKind::UnknownUefi(0x80000000))
            .next()
            .unwrap()
            .start as usize
            + self.inner.offset
    }

    fn end(&self) -> usize {
        self.start() + self.inner.len
    }
}

pub struct Modules {
    inner: &'static info::Modules,
    regions: &'static info::MemoryRegions,
    index: usize,
}

impl Iterator for Modules {
    type Item = Module;

    fn next(&mut self) -> Option<Self::Item> {
        let module = self.inner.get(self.index)?;
        let module = Module {
            inner: *module,
            regions: self.regions,
        };
        self.index += 1;
        Some(module)
    }
}

impl crate::BootInformation for &'static bootloader_api::BootInfo {
    type MemoryArea<'a> = MemoryArea;
    type MemoryAreas<'a> = MemoryAreas;

    type ElfSection<'a> = &'a info::ElfSection;
    type ElfSections<'a> = core::slice::Iter<'a, info::ElfSection>;

    type Module<'a> = Module;
    type Modules<'a> = Modules;

    fn start(&self) -> VirtualAddress {
        VirtualAddress::new(*self as *const _ as usize).expect("invalid boot info virtual address")
    }

    fn size(&self) -> usize {
        self.size
    }

    // The bootloader creates two memory regions with the bootloader type. The first
    // one always starts at 0x1000 and contains the page table, boot info, etc. The
    // second one starts at some other address and contains the nano_core elf file.
    // It is the same size as the nano_core elf file.

    fn kernel_memory_range(&self) -> Result<Range<PhysicalAddress>, &'static str> {
        use crate::ElfSection;

        let start = PhysicalAddress::new(
            self.elf_sections()?
                .into_iter()
                .filter(|s| s.flags().contains(ElfSectionFlags::ALLOCATED))
                .map(|s| s.start())
                .min()
                .ok_or("couldn't find kernel start address")? as usize,
        )
        .ok_or("kernel physical start address was invalid")?;

        let virtual_end = VirtualAddress::new(
            self.elf_sections()?
                .into_iter()
                .filter(|s| s.flags().contains(ElfSectionFlags::ALLOCATED))
                .map(|s| s.start() + s.size())
                .max()
                .ok_or("couldn't find kernel end address")? as usize,
        )
        .ok_or("kernel virtual end address was invalid")?;
        let physical_end = PhysicalAddress::new(virtual_end.value() - KERNEL_OFFSET)
            .ok_or("kernel physical end address was invalid")?;

        log::info!("start: {start:0x?}, end: {physical_end:0x?}");

        Ok(start..physical_end)
    }

    fn bootloader_info_memory_range(&self) -> Result<Range<PhysicalAddress>, &'static str> {
        let mut iter = self
            .memory_regions
            .iter()
            .filter(|region| region.kind == info::MemoryRegionKind::Bootloader)
            .filter(|region| region.start == 0x1000);

        let bootloader_info_memory_region =
            iter.next().ok_or("no bootloader info memory region")?;
        if iter.next().is_some() {
            Err("multiple potential bootloader memory info memory regions")
        } else {
            // FIXME
            let start = PhysicalAddress::new(bootloader_info_memory_region.start as usize)
                .ok_or("invalid bootloader info start address")?;
            let end = PhysicalAddress::new(bootloader_info_memory_region.end as usize)
                .ok_or("invalid bootloader info end address")?;
            Ok(start..end)
        }
    }

    fn modules_memory_range(&self) -> Result<Range<PhysicalAddress>, &'static str> {
        let area = self
            .memory_regions
            .iter()
            .filter(|region| region.kind == info::MemoryRegionKind::UnknownUefi(0x80000000))
            .next()
            .ok_or("no modules memory region")?;
        let start = PhysicalAddress::new_canonical(area.start as usize);
        let end = PhysicalAddress::new_canonical(area.end as usize);
        Ok(start..end)
    }

    fn stack_memory_range(&self) -> Range<VirtualAddress> {
        let start = VirtualAddress::new(self.stack_start).unwrap();
        let end = VirtualAddress::new(self.stack_end).unwrap();
        start..end
    }

    fn memory_areas(&self) -> Result<Self::MemoryAreas<'_>, &'static str> {
        Ok(MemoryAreas {
            inner: self.memory_regions.iter().peekable(),
        })
    }

    fn elf_sections(&self) -> Result<Self::ElfSections<'_>, &'static str> {
        Ok(self.elf_sections.iter())
    }

    fn modules(&self) -> Self::Modules<'_> {
        Modules {
            inner: &self.modules,
            regions: &self.memory_regions,
            index: 0,
        }
    }

    fn rsdp(&self) -> Option<PhysicalAddress> {
        self.rsdp_addr
            .into_option()
            .map(|address| PhysicalAddress::new_canonical(address as usize))
    }
}
