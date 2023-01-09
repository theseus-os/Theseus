use crate::ElfSectionFlags;
use bootloader_api::info;
use core::{
    iter::{Iterator, Peekable},
    ops::Range,
};
use kernel_config::memory::{KERNEL_OFFSET, KERNEL_STACK_SIZE_IN_PAGES, PAGE_SIZE};
use memory_structs::{PhysicalAddress, VirtualAddress};

// TODO: Ideally this would be defined in nano_core. However, that would
// introduce a circular dependency as the boot information needs the stack size.
/// The total stack size including the guard page, and additional page for the
/// double fault handler stack.
pub const STACK_SIZE: usize = (KERNEL_STACK_SIZE_IN_PAGES + 2) * PAGE_SIZE;

/// A custom memory region kind used by the bootloader for the modules.
const MODULES_MEMORY_KIND: info::MemoryRegionKind = info::MemoryRegionKind::UnknownUefi(0x80000000);

pub struct MemoryRegion {
    start: PhysicalAddress,
    len: usize,
    is_usable: bool,
}

impl From<info::MemoryRegion> for MemoryRegion {
    fn from(info::MemoryRegion { start, end, kind }: info::MemoryRegion) -> Self {
        Self {
            start: PhysicalAddress::new_canonical(start as usize),
            len: (end - start) as usize,
            is_usable: matches!(kind, info::MemoryRegionKind::Usable),
        }
    }
}

impl crate::MemoryRegion for MemoryRegion {
    fn start(&self) -> PhysicalAddress {
        self.start
    }
    
    fn is_usable(&self) -> bool {
        self.is_usable
    }
}

pub struct MemoryRegions {
    inner: Peekable<core::slice::Iter<'static, info::MemoryRegion>>,
}

impl Iterator for MemoryRegions {
    type Item = MemoryRegion;

    fn next(&mut self) -> Option<Self::Item> {
        let mut area: MemoryRegion = (*self.inner.next()?).into();

        // UEFI often separates contiguous memory into separate memory regions. We
        // consolidate them to minimise the number of entries in the frame allocator's
        // reserved and available lists.
        while let Some(next) = self.inner.next_if(|next| {
            let next = MemoryRegion::from(**next);
            area.is_usable == next.is_usable && (area.start + area.len) == next.start
        }) {
            let next = MemoryRegion::from(*next);
            area.len += next.len;
        }

        Some(area)
    }
}

impl<'a> crate::ElfSection for &'a info::ElfSection {
    fn name(&self) -> &str {
        info::ElfSection::name(self)
    }

    fn start(&self) -> VirtualAddress {
        VirtualAddress::new_canonical(self.start)
    }

    fn len(&self) -> usize {
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
        Ok(info::Module::name(&self.inner))
    }

    fn start(&self) -> PhysicalAddress {
        PhysicalAddress::new_canonical(
            self.regions
                .iter()
                .find(|region| region.kind == MODULES_MEMORY_KIND)
                .expect("no modules region")
                .start as usize
                + self.inner.offset,
        )
    }

    fn len(&self) -> usize {
        self.inner.len
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
    type MemoryRegion<'a> = MemoryRegion;
    type MemoryRegions<'a> = MemoryRegions;

    type ElfSection<'a> = &'a info::ElfSection;
    type ElfSections<'a> = core::slice::Iter<'a, info::ElfSection>;

    type Module<'a> = Module;
    type Modules<'a> = Modules;

    fn start(&self) -> Option<VirtualAddress> {
        VirtualAddress::new(*self as *const _ as usize)
    }

    fn len(&self) -> usize {
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
                .ok_or("couldn't find kernel start address")?
                .value(),
        )
        .ok_or("kernel physical start address was invalid")?;
        let virtual_end = self
            .elf_sections()?
            .into_iter()
            .filter(|s| s.flags().contains(ElfSectionFlags::ALLOCATED))
            .map(|s| s.start() + s.len())
            .max()
            .ok_or("couldn't find kernel end address")?;
        let physical_end = PhysicalAddress::new(virtual_end.value() - KERNEL_OFFSET)
            .ok_or("kernel physical end address was invalid")?;

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
            .find(|region| region.kind == MODULES_MEMORY_KIND)
            .ok_or("no modules memory region")?;
        let start = PhysicalAddress::new_canonical(area.start as usize);
        let end = PhysicalAddress::new_canonical(area.end as usize);
        Ok(start..end)
    }

    fn memory_regions(&self) -> Result<Self::MemoryRegions<'_>, &'static str> {
        Ok(MemoryRegions {
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

    fn stack_size(&self) -> Result<usize, &'static str> {
        Ok(STACK_SIZE)
    }
}
