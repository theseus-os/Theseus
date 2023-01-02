use crate::ElfSectionFlags;
use core::{iter::Iterator, ops::Range};
use kernel_config::memory::{KERNEL_OFFSET, KERNEL_STACK_SIZE_IN_PAGES, PAGE_SIZE};
use memory_structs::{PhysicalAddress, VirtualAddress};
use uefi_bootloader_api;

// TODO: Ideally this would be defined in nano_core. However, that would
// introduce a circular dependency as the boot information needs the stack size.
/// The total stack size including the guard page, and additional page for the
/// double fault handler stack.
pub const STACK_SIZE: usize = (KERNEL_STACK_SIZE_IN_PAGES + 2) * PAGE_SIZE;

/// A custom memory region kind used by the bootloader for the modules.
const MODULES_MEMORY_KIND: uefi_bootloader_api::MemoryRegionKind =
    uefi_bootloader_api::MemoryRegionKind::UnknownUefi(0x80000000);

impl<'a> crate::MemoryRegion for &'a uefi_bootloader_api::MemoryRegion {
    fn start(&self) -> PhysicalAddress {
        PhysicalAddress::new_canonical(self.start)
    }

    fn len(&self) -> usize {
        self.len
    }

    fn is_usable(&self) -> bool {
        matches!(self.kind, uefi_bootloader_api::MemoryRegionKind::Usable)
    }
}

impl<'a> crate::ElfSection for &'a uefi_bootloader_api::ElfSection {
    fn name(&self) -> &str {
        uefi_bootloader_api::ElfSection::name(self)
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

#[derive(Debug)]
pub struct Module {
    inner: uefi_bootloader_api::Module,
    regions: &'static uefi_bootloader_api::MemoryRegions,
}

impl crate::Module for Module {
    fn name(&self) -> Result<&str, &'static str> {
        Ok(uefi_bootloader_api::Module::name(&self.inner))
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
    inner: &'static uefi_bootloader_api::Modules,
    regions: &'static uefi_bootloader_api::MemoryRegions,
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

impl crate::BootInformation for &'static uefi_bootloader_api::BootInformation {
    type MemoryRegion<'a> = &'a uefi_bootloader_api::MemoryRegion;
    type MemoryRegions<'a> = core::slice::Iter<'a, uefi_bootloader_api::MemoryRegion>;

    type ElfSection<'a> = &'a uefi_bootloader_api::ElfSection;
    type ElfSections<'a> = core::slice::Iter<'a, uefi_bootloader_api::ElfSection>;

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
        // The bootloader already marked the info memory regions as reserved.
        // TODO: Improve function name.
        Ok(PhysicalAddress::zero()..PhysicalAddress::zero())
    }

    fn modules_memory_range(&self) -> Result<Range<PhysicalAddress>, &'static str> {
        let area = self
            .memory_regions
            .iter()
            .find(|region| region.kind == MODULES_MEMORY_KIND)
            .ok_or("no modules memory region")?;
        let start = PhysicalAddress::new_canonical(area.start as usize);
        let end = start + area.len;
        Ok(start..end)
    }

    fn memory_regions(&self) -> Result<Self::MemoryRegions<'_>, &'static str> {
        Ok(self.memory_regions.iter())
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
        self.rsdp_address
            .map(|address| PhysicalAddress::new_canonical(address))
    }

    fn stack_size(&self) -> Result<usize, &'static str> {
        Ok(STACK_SIZE)
    }
}
