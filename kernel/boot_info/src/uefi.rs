use bootloader_api::info;
use core::{
    iter::{Iterator, Peekable},
    ops::Range,
};
use kernel_config::memory::KERNEL_OFFSET;
use memory_structs::{Frame, Page, PhysicalAddress, VirtualAddress};
use xmas_elf::ElfFile;

use crate::{ElfSectionFlags, MemoryAreaType};

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

    fn flags(&self) -> ElfSectionFlags {
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

pub struct Mapping {
    page: Page,
    frame: Frame,
}

impl crate::Mapping for Mapping {
    fn page(&self) -> Page {
        self.page
    }

    fn frame(&self) -> Frame {
        self.frame
    }
}

pub struct Mappings {
    address: usize,
    mapper: (),
}

impl Iterator for Mappings {
    type Item = Mapping;

    fn next(&mut self) -> Option<Self::Item> {
        todo!();
    }
}

impl crate::BootInformation for &'static bootloader_api::BootInfo {
    type MemoryArea<'a> = MemoryArea;
    type MemoryAreas<'a> = MemoryAreas;

    type ElfSection<'a> = &'a info::ElfSection;
    type ElfSections<'a> = core::slice::Iter<'a, info::ElfSection>;

    type Module<'a> = Module;
    type Modules<'a> = Modules;

    // type Mapping<'a> = Mapping;

    // type Mappings<'a> = Mappings;

    fn address(&self) -> VirtualAddress {
        let rr = self as *const _;
        let r = *self as *const _;
        log::info!("rr: {rr:#p}");
        log::info!("r: {r:#p}");
        VirtualAddress::new(*self as *const _ as usize).unwrap()
    }

    fn size(&self) -> usize {
        self.size
    }

    // FIXME: Is using physical addresses ok?

    // The bootloader creates two memory regions with the bootloader type. The first
    // one always starts at 0x1000 and contains the page table, boot info, etc. The
    // second one starts at some other address and contains the nano_core elf file.
    // It is the same size as the nano_core elf file.

    fn kernel_memory_range(&self) -> Result<Range<PhysicalAddress>, &'static str> {
        use crate::MemoryArea;
        for region in self.memory_regions.iter() {
            log::info!(
                "start: {:0x?}, end: {:0x?}, size: {:0x?}, ty: {:?}",
                region.start,
                region.end,
                (region.end - region.start),
                region.kind
            );
        }
        let count = self
            .memory_regions
            .iter()
            .filter(|region| region.kind == info::MemoryRegionKind::Usable)
            .count();
        log::info!("count: {:?}", count);

        for region in self.memory_areas().unwrap() {
            log::info!(
                "start: {:0x?}, size: {:0x?}, ty: {:?}",
                region.start(),
                region.size(),
                region.ty()
            );
        }
        let count = self
            .memory_areas()
            .unwrap()
            .filter(|area| area.ty() == MemoryAreaType::Available)
            .count();
        log::info!("count: {:?}", count);

        for module in self.modules() {
            use crate::Module;
            log::info!("name: {}", module.name().unwrap());
        }

        // let mut iter = self
        //     .memory_regions
        //     .iter()
        //     .filter(|region| region.kind == info::MemoryRegionKind::Bootloader)
        //     .filter(|region| region.start != 0x1000);

        // let kernel_memory_region = iter.next().ok_or("no kernel memory region")?;

        // if iter.next().is_some() {
        //     Err("multiple potential kernel memory regions")
        // } else {
        //     let start = PhysicalAddress::new(kernel_memory_region.start as usize)
        //         .ok_or("invalid kernel start address")?;
        //     let end = PhysicalAddress::new(kernel_memory_region.end as usize)
        //         .ok_or("invalid kernel end address")?;
        //     Ok(start..end)
        // }

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
        // The memory subsystem uses this to reserve the modules' memory. However, the
        // bootloader already does this for us.
        Ok(PhysicalAddress::zero()..PhysicalAddress::zero())
    }

    fn memory_areas(&self) -> Result<Self::MemoryAreas<'_>, &'static str> {
        Ok(MemoryAreas {
            inner: self.memory_regions.iter().peekable(),
        })
    }

    // FIXME: This is not static. It must not be used after the new page table is
    // swapped in.
    fn elf_sections(&self) -> Result<Self::ElfSections<'static>, &'static str> {
        // let kernel_memory_range = self.kernel_memory_range()?;
        // let physical_memory_offset = self
        //     .physical_memory_offset
        //     .into_option()
        //     .ok_or("physical memory offset not given")?;

        // let kernel_virtual_start =
        //     (usize::from(kernel_memory_range.start) + physical_memory_offset as
        // usize) as *const u8; let kernel_length =
        //     usize::from(kernel_memory_range.end) -
        // usize::from(kernel_memory_range.start);

        // let kernel_bytes: &'static [u8] =
        //     unsafe { core::slice::from_raw_parts(kernel_virtual_start, kernel_length)
        // };

        // let file = xmas_elf::ElfFile::new(kernel_bytes)?;
        // Ok(ElfSections { file, index: 0 })

        Ok(self.elf_sections.iter())
    }

    fn modules(&self) -> Self::Modules<'_> {
        Modules {
            inner: &self.modules,
            regions: &self.memory_regions,
            index: 0,
        }
    }

    fn stack_range(&self) -> Range<VirtualAddress> {
        let start = VirtualAddress::new(self.stack_start).unwrap();
        let end = VirtualAddress::new(self.stack_end).unwrap();
        start..end
    }

    fn rsdp(&self) -> Option<usize> {
        self.rsdp_addr.into_option().map(|address| address as usize)
    }
}
