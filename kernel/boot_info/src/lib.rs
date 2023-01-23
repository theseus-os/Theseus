//! This crate provides an abstraction over multiboot2 and UEFI boot
//! information.
//!
//! It allows the kernel's initialisation to be the same, regardless of whether
//! it was booted using BIOS or UEFI.

#![feature(type_alias_impl_trait)]
#![no_std]

#[cfg(feature = "multiboot2")]
pub mod multiboot2;

#[cfg(feature = "uefi")]
pub mod uefi;

use log::{debug, error};
use core::iter::Iterator;
use memory_structs::{PhysicalAddress, VirtualAddress};
use pte_flags::PteFlags;

#[cfg(target_arch = "x86_64")]
use kernel_config::memory::KERNEL_OFFSET;

pub trait MemoryRegion {
    /// Returns the region's starting physical address.
    fn start(&self) -> PhysicalAddress;

    /// Returns the region's length.
    fn len(&self) -> usize;

    /// Returns whether the region is empty.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns whether the region can be used by the frame allocator.
    fn is_usable(&self) -> bool;
}

pub trait ElfSection {
    /// Returns the section's name.
    fn name(&self) -> &str;

    /// Returns the section's starting virtual address.
    fn start(&self) -> VirtualAddress;

    /// Returns the section's length in memory, as opposed to its length in the
    /// ELF file.
    fn len(&self) -> usize;

    /// Returns whether the section is empty.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns the section's flags.
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
    /// Returns the module's name.
    fn name(&self) -> Result<&str, &'static str>;

    /// Returns the module's starting physical address.
    fn start(&self) -> PhysicalAddress;

    /// Returns the module's length.
    fn len(&self) -> usize;

    /// Returns whether the module is empty.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[derive(Debug)]
pub struct ReservedMemoryRegion {
    pub start: PhysicalAddress,
    pub len: usize,
}

pub trait BootInformation: 'static {
    type MemoryRegion<'a>: MemoryRegion;
    type MemoryRegions<'a>: Iterator<Item = Self::MemoryRegion<'a>>;

    type ElfSection<'a>: ElfSection;
    type ElfSections<'a>: Iterator<Item = Self::ElfSection<'a>>;

    type Module<'a>: Module;
    type Modules<'a>: Iterator<Item = Self::Module<'a>>;

    type AdditionalReservedMemoryRegions: Iterator<Item = ReservedMemoryRegion>;

    /// Returns the boot information's starting virtual address.
    fn start(&self) -> Option<VirtualAddress>;
    /// Returns the boot information's length.
    fn len(&self) -> usize;

    /// Returns whether the boot information is empty.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns memory regions describing the physical memory.
    fn memory_regions(&self) -> Result<Self::MemoryRegions<'_>, &'static str>;
    /// Returns the kernel's ELF sections.
    fn elf_sections(&self) -> Result<Self::ElfSections<'_>, &'static str>;
    /// Returns the modules found in the kernel image.
    fn modules(&self) -> Self::Modules<'_>;

    /// Returns additional reserved memory regions that aren't included in
    /// the list of regions returned by [`memory_regions`].
    fn additional_reserved_memory_regions(
        &self,
    ) -> Result<Self::AdditionalReservedMemoryRegions, &'static str>;

    /// Returns the end of the kernel's image in memory.
    fn kernel_end(&self) -> Result<VirtualAddress, &'static str>;

    /// Returns the RSDP if it was provided by the bootloader.
    fn rsdp(&self) -> Option<PhysicalAddress>;

    /// Returns the stack size in bytes.
    fn stack_size(&self) -> Result<usize, &'static str>;
}

/// The address bounds and mapping flags of a section's memory region.
#[derive(Debug)]
pub struct SectionMemoryBounds {
    /// The starting virtual address and physical address.
    pub start: (VirtualAddress, PhysicalAddress),
    /// The ending virtual address and physical address.
    pub end: (VirtualAddress, PhysicalAddress),
    /// The page table entry flags that should be used for mapping this section.
    pub flags: PteFlags,
}

/// The address bounds and flags of the initial kernel sections that need mapping. 
/// 
/// Individual sections in the kernel's ELF image are combined here according to their flags,
/// as described below, but some are kept separate for the sake of correctness or ease of use.
/// 
/// It contains three main items, in which each item includes all sections that have identical flags:
/// * The `text` section bounds cover all sections that are executable.
/// * The `rodata` section bounds cover those that are read-only (.rodata, .gcc_except_table, .eh_frame).
///   * The `rodata` section also includes thread-local storage (TLS) areas (.tdata, .tbss) if they exist,
///     because they can be mapped using the same page table flags.
/// * The `data` section bounds cover those that are writable (.data, .bss).
#[derive(Debug)]
pub struct AggregatedSectionMemoryBounds {
   pub init:        SectionMemoryBounds,
   pub text:        SectionMemoryBounds,
   pub rodata:      SectionMemoryBounds,
   pub data:        SectionMemoryBounds,
}

/// Converts the given multiboot2 section's flags into `PteFlags`.
fn convert_to_pte_flags(section: &impl ElfSection) -> PteFlags {
    PteFlags::new()
        .valid(section.flags().contains(ElfSectionFlags::ALLOCATED))
        .writable(section.flags().contains(ElfSectionFlags::WRITABLE))
        .executable(section.flags().contains(ElfSectionFlags::EXECUTABLE))
}

/// Finds the addresses in memory of the main kernel sections, as specified by the given boot information. 
/// 
/// Returns the following tuple, if successful:
///  * The combined size and address bounds of key sections, e.g., .text, .rodata, .data.
///    Each of the these section bounds is aggregated to cover the bounds and sizes of *all* sections 
///    that share the same page table mapping flags and can thus be logically combined.
///  * The list of all individual sections found. 
pub fn find_section_memory_bounds<F>(boot_info: &impl BootInformation, translate: F) -> Result<(AggregatedSectionMemoryBounds, [Option<SectionMemoryBounds>; 32]), &'static str>
where
    F: Fn(VirtualAddress) -> Option<PhysicalAddress>,
{
    let mut index = 0;
    let mut init_start:        Option<(VirtualAddress, PhysicalAddress)> = None;
    let mut init_end:          Option<(VirtualAddress, PhysicalAddress)> = None;
    let mut text_start:        Option<(VirtualAddress, PhysicalAddress)> = None;
    let mut text_end:          Option<(VirtualAddress, PhysicalAddress)> = None;
    let mut rodata_start:      Option<(VirtualAddress, PhysicalAddress)> = None;
    let mut rodata_end:        Option<(VirtualAddress, PhysicalAddress)> = None;
    let mut data_start:        Option<(VirtualAddress, PhysicalAddress)> = None;
    let mut data_end:          Option<(VirtualAddress, PhysicalAddress)> = None;

    let mut init_flags:        Option<PteFlags> = None;
    let mut text_flags:        Option<PteFlags> = None;
    let mut rodata_flags:      Option<PteFlags> = None;
    let mut data_flags:        Option<PteFlags> = None;

    let mut sections_memory_bounds: [Option<SectionMemoryBounds>; 32] = Default::default();

    // map the allocated kernel text sections
    for section in boot_info.elf_sections()? {
        // skip sections that don't need to be loaded into memory
        if section.len() == 0
            || !section.flags().contains(ElfSectionFlags::ALLOCATED)
            || section.name().starts_with(".debug")
        {
            continue;
        }

        debug!("Looking at loaded section {} at {:#X}, size {:#X}", section.name(), section.start(), section.len());
        let flags = convert_to_pte_flags(&section);

        let start_virt_addr = VirtualAddress::new(section.start().value())
            .ok_or("section had invalid starting virtual address")?;
        let start_phys_addr = translate(start_virt_addr)
            .ok_or("couldn't translate section's starting virtual address")?;

        #[cfg(target_arch = "x86_64")]
        if start_virt_addr.value() < KERNEL_OFFSET {
            // special case to handle the first section only
            start_virt_addr += KERNEL_OFFSET;
        }

        let end_virt_addr = start_virt_addr + section.len();
        let end_phys_addr = start_phys_addr + section.len();

        // The linker script (linker_higher_half.ld) defines the following order of sections:
        // |------|-------------------|-------------------------------|
        // | Sec  |    Sec Name       |    Description / purpose      |
        // | Num  |                   |                               |
        // |------|---------------------------------------------------|
        // | (1)  | .init             | start of executable pages     |
        // | (2)  | .text             | end of executable pages       |
        // | (3)  | .rodata           | start of read-only pages      |
        // | (4)  | .eh_frame         | part of read-only pages       |
        // | (5)  | .gcc_except_table | part/end of read-only pages   |
        // | (6)  | .tdata            | part/end of read-only pages   |
        // | (7)  | .tbss             | part/end of read-only pages   |
        // | (8)  | .data             | start of read-write pages     | 
        // | (9)  | .bss              | end of read-write pages       |
        // | (10) | .page_table       | separate .data-like section   |
        // | (11) | .stack            | separate .data-like section   |
        // |------|-------------------|-------------------------------|
        //
        // Note that we combine the TLS data sections (.tdata and .tbss) into the read-only pages,
        // because they contain read-only initializer data "images" for each TLS area.
        // In fact, .tbss can be completedly ignored because it represents a read-only data image of all zeroes,
        // so there's no point in keeping it around.
        //
        // Those are the only sections we care about; we ignore subsequent `.debug_*` sections (and .got).
        let static_str_name = match section.name() {
            ".init" => {
                init_start = Some((start_virt_addr, start_phys_addr));
                init_end = Some((end_virt_addr, end_phys_addr));
                init_flags = Some(flags);
                "nano_core .init"
            } 
            ".text" => {
                text_start = Some((start_virt_addr, start_phys_addr));
                text_end = Some((end_virt_addr, end_phys_addr));
                text_flags = Some(flags);
                "nano_core .text"
            }
            ".rodata" => {
                rodata_start = Some((start_virt_addr, start_phys_addr));
                rodata_flags = Some(flags);

                #[cfg(target_arch = "aarch64")]
                rodata_end = Some((end_virt_addr, end_phys_addr));

                "nano_core .rodata"
            }
            ".eh_frame" => {
                "nano_core .eh_frame"
            }
            ".gcc_except_table" => {
                rodata_end   = Some((end_virt_addr, end_phys_addr));
                rodata_flags = Some(flags);
                "nano_core .gcc_except_table"
            }
            // The following four sections are optional: .tdata, .tbss, .data, .bss.
            ".tdata" => {
                rodata_end = Some((end_virt_addr, end_phys_addr));
                "nano_core .tdata"
            }
            ".tbss" => {
                // Ignore .tbss (see above) because it is a read-only section of all zeroes.
                debug!("     no need to map kernel section \".tbss\", it contains no content");
                continue;
            }
            ".data" => {
                data_start.get_or_insert((start_virt_addr, start_phys_addr));
                data_end = Some((end_virt_addr, end_phys_addr));
                data_flags = Some(flags);
                "nano_core .data"
            }
            ".bss" => {
                data_start.get_or_insert((start_virt_addr, start_phys_addr));
                data_end = Some((end_virt_addr, end_phys_addr));
                data_flags = Some(flags);
                "nano_core .bss"
            }
            // This appears when compiling for BIOS.
            ".page_table" | ".stack" => {
                debug!("     no need to map this section, it is mapped separately later");
                continue;
            }
            // This appears when compiling for UEFI.
            ".bootloader-config" => {
                // TODO: Ideally we'd mark .bootloader-config as not allocated
                // so the bootloader doesn't load it.
                debug!("     no need to map this section, it is only used by the bootloader for config.");
                continue;
            }
            _ =>  {
                error!("Section {} at {:#X}, size {:#X} was not an expected section", 
                        section.name(), section.start(), section.len());
                return Err("Kernel ELF Section had an unexpected name");
            }
        };
        debug!("     will map kernel section {:?} as {:?} at vaddr: {:#X}, size {:#X} bytes", section.name(), static_str_name, start_virt_addr, section.len());

        sections_memory_bounds[index] = Some(SectionMemoryBounds {
            start: (start_virt_addr, start_phys_addr),
            end: (end_virt_addr, end_phys_addr),
            flags,
        });

        index += 1;
    }

    let init_start         = init_start       .ok_or("Couldn't find start of .init section")?;
    let init_end           = init_end         .ok_or("Couldn't find end of .init section")?;
    let text_start         = text_start       .ok_or("Couldn't find start of .text section")?;
    let text_end           = text_end         .ok_or("Couldn't find end of .text section")?;
    let rodata_start       = rodata_start     .ok_or("Couldn't find start of .rodata section")?;
    let rodata_end         = rodata_end       .ok_or("Couldn't find end of .rodata section")?;
    let data_start         = data_start       .ok_or("Couldn't find start of .data section")?;
    let data_end           = data_end         .ok_or("Couldn't find start of .data section")?;
     
    let init_flags    = init_flags  .ok_or("Couldn't find .init section flags")?;
    let text_flags    = text_flags  .ok_or("Couldn't find .text section flags")?;
    let rodata_flags  = rodata_flags.ok_or("Couldn't find .rodata section flags")?;
    let data_flags    = data_flags  .ok_or("Couldn't find .data section flags")?;

    let init = SectionMemoryBounds {
        start: init_start,
        end: init_end,
        flags: init_flags,
    };
    let text = SectionMemoryBounds {
        start: text_start,
        end: text_end,
        flags: text_flags,
    };
    let rodata = SectionMemoryBounds {
        start: rodata_start,
        end: rodata_end,
        flags: rodata_flags,
    };
    let data = SectionMemoryBounds {
        start: data_start,
        end: data_end,
        flags: data_flags,
    };

    let aggregated_sections_memory_bounds = AggregatedSectionMemoryBounds {
        init,
        text,
        rodata,
        data,
    };
    Ok((aggregated_sections_memory_bounds, sections_memory_bounds))
}
