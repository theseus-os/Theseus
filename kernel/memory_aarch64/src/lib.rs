//! This crate implements the virtual memory subsystem interfaces for Theseus on aarch64.
//! `memory` uses this crate to get the memory layout and do other arch-specific operations on aarch64.  
//! 
//! This is the top-level arch-specific memory crate. 
//! All arch-specific definitions for memory system are exported from this crate.

#![no_std]

use cortex_a::asm::barrier;
use cortex_a::registers::{MAIR_EL1, SCTLR_EL1, TCR_EL1, TTBR0_EL1};
use tock_registers::interfaces::{Readable, Writeable, ReadWriteable};

use memory_structs::{PhysicalAddress, VirtualAddress};
use log::{debug, error};
use pte_flags::PteFlags;
use boot_info::{BootInformation, ElfSection};
use kernel_config::memory::KERNEL_OFFSET;

#[cfg(any(target_arch = "aarch64", doc))]
use core::arch::asm;

const THESEUS_ASID: u16 = 0;

#[cfg(any(target_arch = "aarch64", doc))]
/// Flushes the specific virtual address in TLB.
///
/// TLBI => tlb invalidate instruction
/// "va" => all translations at execution level
///         using the supplied address
/// "e1" => execution level
pub fn tlb_flush_virt_addr(vaddr: VirtualAddress) {
    #[cfg(target_arch = "aarch64")]
    unsafe { asm!("tlbi vae1, {}", in(reg) vaddr.value()) };
}

#[cfg(any(target_arch = "aarch64", doc))]
/// Flushes all TLB entries with Theseus' ASID (=0).
///
/// TLBI => tlb invalidate instruction
/// "asid" => all entries with specific ASID
/// "e1" => execution level
pub fn tlb_flush_by_theseus_asid() {
    #[cfg(target_arch = "aarch64")]
    unsafe { asm!("tlbi aside1, {:x}", in(reg) THESEUS_ASID) };
}

#[cfg(any(target_arch = "aarch64", doc))]
pub use tlb_flush_by_theseus_asid as tlb_flush_all;

/// Returns the current top-level page table address.
///
/// We use TTBR0 in Theseus to store the
/// top-level page table, so this function
/// reads that register.
pub fn get_p4() -> PhysicalAddress {
    PhysicalAddress::new_canonical(
        TTBR0_EL1.get_baddr() as usize
    )
}

/// Disable the MMU using aarch64 registers
///
/// This uses the `SCTLR_EL1` register.
///
/// When the MMU is disabled, the CPU acts as
/// if a full-address-space identity mapping
/// was active.
pub fn disable_mmu() {
    SCTLR_EL1.modify(SCTLR_EL1::M::Disable);
    unsafe { barrier::isb(barrier::SY) };
}

/// Enable the MMU using aarch64 registers
///
/// This uses the `SCTLR_EL1` register.
///
/// When the MMU is disabled, the CPU acts as
/// if a full-address-space identity mapping
/// was active. When it's enabled, the TTB0_EL1
/// register is expected to point to a valid
/// page table (using its physical address).
pub fn enable_mmu() {
    SCTLR_EL1.modify(SCTLR_EL1::M::Enable);
    unsafe { barrier::isb(barrier::SY) };
}

/// Configures paging for Theseus.
///
/// ## Resulting Configuration
/// * MAIR slot 0 is for cacheable normal DRAM
/// * MAIR slot 1 is for non-cacheable device memory
/// * A physical address is 48-bits long
/// * Only the first translation unit is used (TTBR0)
/// * The Page Size is 4KiB
/// * The ASID size is 8 bits.
/// * The MMU is allowed to update the DIRTY and ACCESSED
///   flags in a page table entry.
pub fn configure_translation_registers() {
    unsafe {
        // The MAIR register holds up to 8 memory profiles;
        // each profile describes cacheability of the memory.
        // In Theseus, we currently use two profiles: one for
        // device memory (non-cacheable) and one for normal
        // memory (the usual RAM memory, cacheable).
        //
        // For more information on MAIR, See section D17.2.97
        // of [DDI0487l.a](https://l0.pm/arm-ddi0487l.a.pdf).
        MAIR_EL1.write(
            // Attribute 1 - Device.
            MAIR_EL1::Attr1_Device::nonGathering_nonReordering_EarlyWriteAck +

            // Attribute 0 - Cacheable normal DRAM.
            MAIR_EL1::Attr0_Normal_Outer::WriteBack_NonTransient_ReadWriteAlloc +
            MAIR_EL1::Attr0_Normal_Inner::WriteBack_NonTransient_ReadWriteAlloc,
        );

        // The translation control register contains most of
        // parameters for address translation; these parameters
        // notably define the format for page table entry flags.
        TCR_EL1.write(
            // TCR_EL1::DS is not exposed by `cortex_a`.
            // by default (cleared) it means we cannot
            // use 52-bit output-address mode.

            // Whether to use the top-byte of virtual
            // addresses for tagged addresses ("Ignored")
            // or to use them in the page table walk ("Used").
            // With our four-level paging, however, the top-byte
            // is not used for page table walks anyway.
              TCR_EL1::TBI0::Used
            // | TCR_EL1::TBI1::Used

            // Translation Granule Size = Page Size
            // => four kilobytes
            + TCR_EL1::TG0::KiB_4
            // + TCR_EL1::TG1::KiB_4

            // These fields could only be used if we had access
            // to the DS field, and if we set it to one. Indeed,
            // when DS=1, the shareability fields of page
            // descriptors are replaced by some bits of the output
            // address; the shareability is constant for the whole
            // page table: one for TTBR0 and one for TTBR1.
            // + TCR_EL1::SH0::Inner
            // + TCR_EL1::SH1::Inner

            // ASID Size. The upper 8 bits of TTBR0_EL1 and
            // TTBR1_EL1 are ignored by hardware for every
            // purpose except reading back the register, and
            // are treated as if they are all zeros for when
            // used for allocation and matching entries in the TLB.
            + TCR_EL1::AS::ASID8Bits

            // We currently output 48 bits of physical
            // address on aarch64.
            + TCR_EL1::IPS::Bits_48

            // Translation table walk disable. This bit controls
            // whether a translation table walk is performed on
            // a TLB miss.
            + TCR_EL1::EPD0::EnableTTBR0Walks
            // + TCR_EL1::EPD1::EnableTTBR1Walks

            // From which TTBR to read the ASID, when comparing the
            // current address space with the one from an address.
            + TCR_EL1::A1::TTBR0

            // Controls the size of the memory region addressed
            // by page table walks. We have to write 64 - (max
            // number of bits in a region address): 64 - 48 = 16.
            + TCR_EL1::T0SZ.val(16)
            // + TCR_EL1::T1SZ.val(16)

            // I (Nathan) don't understand when these flags are used at the moment
            // + TCR_EL1::ORGN0::WriteBack_ReadAlloc_WriteAlloc_Cacheable
            // + TCR_EL1::ORGN1::NonCacheable
            // + TCR_EL1::IRGN0::WriteBack_ReadAlloc_WriteAlloc_Cacheable
            // + TCR_EL1::IRGN1::NonCacheable

            // Allow the MMU to update the ACCESSED flag.
            + TCR_EL1::HA::Enable

            // Allow the MMU to update the DIRTY flag.
            + TCR_EL1::HD::Enable
        );

        barrier::isb(barrier::SY);
    }
}

/// Sets the given `page_table` as active by updating the TTBR0 register.
///
/// An ASID of all zeros is also configured.
pub fn set_as_active_page_table_root(page_table: PhysicalAddress) {
    unsafe {
        let page_table_addr = page_table.value() as u64;
        TTBR0_EL1.write(
              TTBR0_EL1::ASID.val(THESEUS_ASID as u64)
            + TTBR0_EL1::BADDR.val(page_table_addr >> 1)
        );

        barrier::isb(barrier::SY);
    }
}

/// See [`read_mmu_config`]
#[repr(C)]
pub struct MmuConfig {
    ttbr0_el1: u64,
    mair_el1: u64,
    tcr_el1: u64,
    sctlr_el1: u64,
}

/// Reads the current MMU configuration of the current CPU core,
/// including the following system registers:
/// - `TTBR0_EL1`,
/// - `MAIR_EL1`,
/// - `TCR_EL1`,
/// - `SCTLR_EL1`
///
/// This configuration can be applied using [`asm_set_mmu_config_x2_x3`].
///
/// This is intended for use in `multicore_bringup`.
pub fn read_mmu_config() -> MmuConfig {
    MmuConfig {
        ttbr0_el1: TTBR0_EL1.get(),
        mair_el1: MAIR_EL1.get(),
        tcr_el1: TCR_EL1.get(),
        sctlr_el1: SCTLR_EL1.get(),
    }
}

/// Configures the MMU based on the pointer to a MmuConfig,
/// in x2. This function makes use of x3 too. If the MMU was
/// enabled on the origin core, it will be enabled by this
/// on the target core.
///
/// This is intended for use in `multicore_bringup`.
#[macro_export]
macro_rules! asm_set_mmu_config_x2_x3 {
    () => (
        // Save all general purpose registers into the previous task.
        r#"
            ldr x3, [x2, 0]
            msr ttbr0_el1, x3
            ldr x3, [x2, 1*8]
            msr mair_el1, x3
            ldr x3, [x2, 2*8]
            msr tcr_el1, x3
            ldr x3, [x2, 3*8]
            msr sctlr_el1, x3
        "#
    );
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
            || !section.flags().contains(boot_info::ElfSectionFlags::ALLOCATED)
            || section.name().starts_with(".debug")
        {
            continue;
        }

        debug!("Looking at loaded section {} at {:#X}, size {:#X}", section.name(), section.start(), section.len());
        let flags = convert_to_pte_flags(&section);

        let mut start_virt_addr = VirtualAddress::new(section.start().value())
            .ok_or("section had invalid starting virtual address")?;
        let start_phys_addr = translate(start_virt_addr)
            .ok_or("couldn't translate section's starting virtual address")?;

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
        // | (8)  | .cls              | part/end of read-only pages   |
        // | (9)  | .data             | start of read-write pages     |
        // | (10) | .bss              | end of read-write pages       |
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
            // The following five sections are optional: .tdata, .tbss, .cls, .data, .bss.
            ".tdata" => {
                rodata_end = Some((end_virt_addr, end_phys_addr));
                "nano_core .tdata"
            }
            ".tbss" => {
                // Ignore .tbss (see above) because it is a read-only section of all zeroes.
                debug!("     no need to map kernel section \".tbss\", it contains no content");
                continue;
            }
            ".cls" => {
                rodata_end = Some((end_virt_addr, end_phys_addr));
                "nano_core .cls"
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

/// Converts the given multiboot2 section's flags into `PteFlags`.
fn convert_to_pte_flags(section: &impl ElfSection) -> PteFlags {
    use boot_info::ElfSectionFlags;
    PteFlags::new()
        .valid(section.flags().contains(ElfSectionFlags::ALLOCATED))
        .writable(section.flags().contains(ElfSectionFlags::WRITABLE))
        .executable(section.flags().contains(ElfSectionFlags::EXECUTABLE))
}
