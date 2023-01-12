use crate::{ElfSectionFlags, ReservedMemoryRegion};
use core::{cmp, iter::Iterator};
use kernel_config::memory::KERNEL_OFFSET;
use memory_structs::{PhysicalAddress, VirtualAddress};

impl<'a> crate::MemoryRegion for &'a multiboot2::MemoryArea {
    fn start(&self) -> PhysicalAddress {
        PhysicalAddress::new_canonical(self.start_address() as usize)
    }

    fn len(&self) -> usize {
        multiboot2::MemoryArea::size(self) as usize
    }

    fn is_usable(&self) -> bool {
        matches!(self.typ(), multiboot2::MemoryAreaType::Available)
    }
}

type MemoryRegionIterator<'a> = impl Iterator<Item = &'a multiboot2::MemoryArea>;

pub struct MemoryRegions<'a> {
    inner: MemoryRegionIterator<'a>,
}

impl<'a> Iterator for MemoryRegions<'a> {
    type Item = &'a multiboot2::MemoryArea;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next()
    }
}

impl crate::ElfSection for multiboot2::ElfSection {
    fn name(&self) -> &str {
        multiboot2::ElfSection::name(self)
    }

    fn start(&self) -> VirtualAddress {
        VirtualAddress::new_canonical(self.start_address() as usize)
    }

    fn len(&self) -> usize {
        multiboot2::ElfSection::size(self) as usize
    }

    fn flags(&self) -> ElfSectionFlags {
        let mut boot_info_flags = ElfSectionFlags::empty();
        let flags = multiboot2::ElfSection::flags(self);

        if flags.contains(multiboot2::ElfSectionFlags::WRITABLE) {
            boot_info_flags |= ElfSectionFlags::WRITABLE;
        }

        if flags.contains(multiboot2::ElfSectionFlags::ALLOCATED) {
            boot_info_flags |= ElfSectionFlags::ALLOCATED;
        }

        if flags.contains(multiboot2::ElfSectionFlags::EXECUTABLE) {
            boot_info_flags |= ElfSectionFlags::EXECUTABLE;
        }

        boot_info_flags
    }
}

impl<'a> crate::Module for &'a multiboot2::ModuleTag {
    fn name(&self) -> Result<&str, &'static str> {
        self.cmdline()
            .map_err(|_| "multiboot2 module cmdline was an invalid UTF-8 sequence")
    }

    fn start(&self) -> PhysicalAddress {
        PhysicalAddress::new_canonical(self.start_address() as usize)
    }

    fn len(&self) -> usize {
        (self.end_address() - self.start_address()) as usize
    }

    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

fn kernel_memory_region(
    boot_info: &multiboot2::BootInformation,
) -> Result<ReservedMemoryRegion, &'static str> {
    use crate::BootInformation;

    // Our linker script specifies that the kernel will have the .init section
    // starting at 1MB and ending at 1MB + .init size and all other kernel sections
    // will start at (KERNEL_OFFSET + 1MB) and end at (KERNEL_OFFSET + 1MB + size).
    // So, the start of the kernel is its physical address, but the end of it is its
    // virtual address... confusing, I know. Thus, kernel_phys_start is the same as
    // kernel_virt_start initially, but we remap them later in paging::init.
    let mut physical_start = usize::MAX;
    let mut virtual_end = 0;

    for section in boot_info
        .elf_sections()?
        .filter(|section| section.is_allocated())
    {
        physical_start = cmp::min(section.start_address() as usize, physical_start);
        virtual_end = cmp::max(section.end_address() as usize, virtual_end);
    }

    let physical_end = PhysicalAddress::new(virtual_end - KERNEL_OFFSET)
        .ok_or("kernel physical end address was invalid")?;

    Ok(ReservedMemoryRegion {
        start: PhysicalAddress::new(physical_start)
            .ok_or("kernel physical start address was invalid")?,
        len: (physical_end - physical_start).value(),
    })
}

fn bootloader_info_memory_region(
    boot_info: &multiboot2::BootInformation,
) -> Result<ReservedMemoryRegion, &'static str> {
    let start = PhysicalAddress::new(boot_info.start_address() - KERNEL_OFFSET)
        .ok_or("invalid bootloader info start address")?;
    let end = PhysicalAddress::new(boot_info.end_address() - KERNEL_OFFSET)
        .ok_or("invalid bootloader info end address")?;
    Ok(ReservedMemoryRegion {
        start,
        len: (end - start).value(),
    })
}

fn modules_memory_region(
    boot_info: &multiboot2::BootInformation,
) -> Result<ReservedMemoryRegion, &'static str> {
    let mut min = usize::MAX;
    let mut max = 0;

    for module in boot_info.module_tags() {
        min = cmp::min(min, module.start_address() as usize);
        max = cmp::max(max, module.end_address() as usize);
    }

    Ok(ReservedMemoryRegion {
        start: PhysicalAddress::new(min).ok_or("invalid modules start address")?,
        len: max - min,
    })
}

impl crate::BootInformation for multiboot2::BootInformation {
    type MemoryRegion<'a> = &'a multiboot2::MemoryArea;
    type MemoryRegions<'a> = MemoryRegions<'a>;

    type ElfSection<'a> = multiboot2::ElfSection;
    type ElfSections<'a> = multiboot2::ElfSectionIter;

    type Module<'a> = &'a multiboot2::ModuleTag;
    type Modules<'a> = multiboot2::ModuleIter<'a>;

    type AdditionalReservedMemoryRegions = core::array::IntoIter<ReservedMemoryRegion, 3>;

    fn start(&self) -> Option<VirtualAddress> {
        VirtualAddress::new(self.start_address())
    }

    fn len(&self) -> usize {
        self.total_size()
    }

    fn memory_regions(&self) -> Result<Self::MemoryRegions<'_>, &'static str> {
        Ok(MemoryRegions {
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

    fn modules(&self) -> Self::Modules<'_> {
        self.module_tags()
    }

    fn additional_reserved_memory_regions(
        &self,
    ) -> Result<Self::AdditionalReservedMemoryRegions, &'static str> {
        Ok([
            kernel_memory_region(self)?,
            bootloader_info_memory_region(self)?,
            modules_memory_region(self)?,
        ]
        .into_iter())
    }

    fn kernel_end(&self) -> Result<VirtualAddress, &'static str> {
        use crate::ElfSection;
        self.elf_sections()?
            .map(|section| section.start() + section.len())
            .max()
            .ok_or("no elf sections")
    }

    fn rsdp(&self) -> Option<PhysicalAddress> {
        self.rsdp_v2_tag()
            .map(|tag| tag.signature())
            .or_else(|| self.rsdp_v1_tag().map(|tag| tag.signature()))
            .and_then(|utf8_result| utf8_result.ok())
            .map(|signature| signature as *const _ as *const () as usize)
            .and_then(PhysicalAddress::new)
    }

    fn stack_size(&self) -> Result<usize, &'static str> {
        use crate::ElfSection;

        self.elf_sections()?
            .filter(|section| section.name() == ".stack")
            .map(|section| {
                let start = section.start();
                let end = start + section.len();
                (end - start).value()
            })
            .next()
            .ok_or("no stack section")
    }
}
