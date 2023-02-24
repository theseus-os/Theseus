use crate::ElfSectionFlags;
use core::iter::{Iterator, Peekable};
use kernel_config::memory::{KERNEL_STACK_SIZE_IN_PAGES, PAGE_SIZE};
use memory_structs::{PhysicalAddress, VirtualAddress};

// TODO: Ideally this would be defined in nano_core. However, that would
// introduce a circular dependency as the boot information needs the stack size.
/// The total stack size including the guard page, and additional page for the
/// double fault handler stack.
pub const STACK_SIZE: usize = (KERNEL_STACK_SIZE_IN_PAGES + 2) * PAGE_SIZE;

/// A custom memory region kind used by the bootloader for the modules.
const MODULES_MEMORY_KIND: uefi_bootloader_api::MemoryRegionKind =
    uefi_bootloader_api::MemoryRegionKind::UnknownUefi(0x80000000);

impl crate::MemoryRegion for uefi_bootloader_api::MemoryRegion {
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

pub struct MemoryRegions<'a> {
    inner: Peekable<core::slice::Iter<'a, uefi_bootloader_api::MemoryRegion>>,
}

impl<'a> Iterator for MemoryRegions<'a> {
    type Item = uefi_bootloader_api::MemoryRegion;

    fn next(&mut self) -> Option<Self::Item> {
        let mut region = *self.inner.next()?;

        // UEFI often separates contiguous memory into separate memory regions. We
        // consolidate them to minimise the number of entries in the frame allocator's
        // reserved and available lists.
        while let Some(next) = self
            .inner
            .next_if(|next| region.kind == next.kind && (region.start + region.len) == next.start)
        {
            region.len += next.len;
        }

        Some(region)
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
                .start
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
    type MemoryRegion<'a> = uefi_bootloader_api::MemoryRegion;
    type MemoryRegions<'a> = MemoryRegions<'a>;

    type ElfSection<'a> = &'a uefi_bootloader_api::ElfSection;
    type ElfSections<'a> = core::slice::Iter<'a, uefi_bootloader_api::ElfSection>;

    type Module<'a> = Module;
    type Modules<'a> = Modules;

    type AdditionalReservedMemoryRegions = core::iter::Empty<crate::ReservedMemoryRegion>;

    fn start(&self) -> Option<VirtualAddress> {
        VirtualAddress::new(*self as *const _ as usize)
    }

    fn len(&self) -> usize {
        self.size
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

    fn additional_reserved_memory_regions(
        &self,
    ) -> Result<Self::AdditionalReservedMemoryRegions, &'static str> {
        Ok(core::iter::empty())
    }

    fn kernel_end(&self) -> Result<VirtualAddress, &'static str> {
        use crate::ElfSection;

        VirtualAddress::new(
            self.elf_sections()?
                .filter(|section| section.flags().contains(ElfSectionFlags::ALLOCATED))
                .filter(|section| section.size > 0)
                .map(|section| section.start + section.size)
                .max()
                .ok_or("couldn't find kernel end address")?
        )
        .ok_or("kernel virtual end address was invalid")
    }

    fn rsdp(&self) -> Option<PhysicalAddress> {
        self.rsdp_address
            .map(PhysicalAddress::new_canonical)
    }

    fn stack_size(&self) -> Result<usize, &'static str> {
        Ok(STACK_SIZE)
    }
}
