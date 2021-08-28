//! Intel VT-d (IOMMU) implementation.
//!
//! [Specification](https://software.intel.com/content/dam/develop/external/us/en/documents-tps/vt-directed-io-spec.pdf)

#![allow(dead_code)]
#![no_std]

extern crate alloc;
extern crate irq_safety;
#[macro_use] extern crate log;
#[macro_use] extern crate static_assertions;
extern crate memory;
extern crate spin;
extern crate volatile;
extern crate zerocopy;
extern crate owning_ref;
#[macro_use] extern crate bitflags;

use spin::Once;
use irq_safety::MutexIrqSafe;
use memory::{MappedPages, PageTable, EntryFlags, PhysicalAddress, allocate_frames_at, allocate_pages};
use owning_ref::BoxRefMut;
use alloc::boxed::Box;

mod regs;
use regs::*;

/// Struct representing IOMMU (TODO: rename since this is specific to Intel VT-d)
pub struct IntelIommu {
    /// Width of host addresses available for DMA
    host_address_width: u8,
    /// PCI segment number associated with this IOMMU
    pci_segment_number: u16,
    /// Register set base address
    register_base_address: PhysicalAddress,
    /// Memory mapped control registers
    regs: BoxRefMut<MappedPages,IntelIommuRegisters>,
}

/// Singleton representing IOMMU (TODO: could there be more than one IOMMU?)
static IOMMU: Once<MutexIrqSafe<IntelIommu>> = Once::new();

/// Initialize the IOMMU driver and hardware.
///
/// # Arguments
/// * `host_address_width`: number of address bits available for DMA
/// * `pci_segment_number`: PCI segment associated with this IOMMU
/// * `register_base_address`: base address of register set
/// * `page_table`: page table to install mapping
pub fn init(host_address_width: u8, pci_segment_number: u16, register_base_address: PhysicalAddress, 
                    page_table: &mut PageTable) -> Result<(), &'static str> {

    info!("IOMMU Init stage 1 begin.");

    // map memory-mapped registers into virtual address space
    let mp = map_iommu_registers(page_table, register_base_address)?;

    // pack into BoxRefMut
    let regs = 
        BoxRefMut::new(Box::new(mp)).try_map_mut(|mp| mp.as_type_mut::<IntelIommuRegisters>(0))?;

    // get the version number
    {
        let contents = regs.version.read();
        let major_ver = (contents >> 4) & 0xf;
        let minor_ver = (contents) & 0xf;
        info!("IOMMU Major Version = {} Minor Version = {}", major_ver, minor_ver);
    }

    // check IOMMU capabilities/extended capabilities
    {
        let c = Capability(regs.cap.read());
        let ec = ExtendedCapability(regs.ecap.read());

        // List capabilities and extended capabilities
        info!("IOMMU Capabilities: {:?}", c);
        info!("IOMMU Extended Capabilities: {:?}", ec);
    }

    // try reading the status register
    {
        let status = GlobalStatus::from_bits_truncate(regs.gstatus.read());
        info!("IOMMU Global Status: {:?}", status);
    }

    // create the "iommu" object
    let iommu = IntelIommu {
        host_address_width,
        pci_segment_number,
        register_base_address,
        regs,
    };

    // initialize the iommu singleton with this object
    IOMMU.call_once(|| {MutexIrqSafe::new(iommu)});

    // Ensure translation is disabled.
    //
    // TODO: This can be removed, it's only purpose is to test that set_command_bit works
    set_command_bit(GlobalCommand::TE, false, |x: GlobalStatus| { ! x.intersects(GlobalStatus::TES) })?;

    info!("IOMMU Init stage 1 complete.");

    Ok(())
}

/// Returns true if IOMMU present in system. Must be called after `iommu::early_init`.
pub fn iommu_present() -> bool {
    IOMMU.is_completed()
}

/// Returns MappedPage(s) to IOMMU memory mapped register region
///
/// # Arguments:
/// * `page_table`: page table to install mapping into
/// * `phys_addr`: physical address of base of mapping (4kB aligned)
fn map_iommu_registers(page_table: &mut PageTable, phys_addr: PhysicalAddress) -> Result<MappedPages, &'static str>
{
    let frames = allocate_frames_at(phys_addr, 1)?;
    let pages = allocate_pages(1).ok_or("Unable to find virtual page!")?;
    let flags = EntryFlags::WRITABLE | EntryFlags::NO_CACHE | EntryFlags::NO_EXECUTE;
    let mapped_pages = page_table.map_allocated_pages_to(pages, frames, flags)?;
    Ok(mapped_pages)
}

/// This function writes a command to the IOMMU Global Command register using
/// the algorithm described in the Intel documentation:
/// 1. Read global status register into temporary variable.
/// 2. Clear all bits in temporary variable that have no effect on command register.
/// 3. Set or clear the corresponding command bit depending on `x`.
/// 4. Write the variable to the command register.
/// 5. Wait until `cond` is met, where `cond` is a function of the status register.
///
/// The condition checked is passed into this function so that the command
/// can be performed as a single atomic operation; there is no need for caller
/// side locking.
///
/// # Arguments:
/// * `c`: command bit to set/clear
/// * `x`: value to set command bit to
/// * `cond`: function which interprets status register and returns true when
///           command has completed.
fn set_command_bit(c: GlobalCommand, x: bool, cond: impl Fn(GlobalStatus) -> bool) -> Result<(), &'static str> {
    let iommu = &mut IOMMU.get().ok_or("IOMMU not initialized!")?.lock();
    let tmp = iommu.regs.gstatus.read();
    let tmp = tmp & 0x96ffffff;
    let bits = c as u32;
    let cmd = if x { tmp | bits } else { tmp & (!bits) };
    iommu.regs.gcommand.write(cmd);
    while !cond(GlobalStatus::from_bits_truncate(iommu.regs.gstatus.read())) {}
    Ok(())
}
