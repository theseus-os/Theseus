//! Code to parse the ACPI tables.
#![no_std]

extern crate alloc;

use alloc::vec::Vec;
use log::{debug, warn, info};
use spin::Mutex;
use memory::{PageTable, PhysicalAddress};
use rsdp::Rsdp;
use acpi_table::AcpiTables;
use acpi_table_handler::acpi_table_handler;


/// The singleton instance of the `AcpiTables` struct,
/// which contains the MappedPages and location of all discovered ACPI tables.
static ACPI_TABLES: Mutex<AcpiTables> = Mutex::new(AcpiTables::empty());

/// Returns a reference to the singleton instance of all ACPI tables 
/// that have been discovered, mapped, and parsed so far.
pub fn get_acpi_tables() -> &'static Mutex<AcpiTables> {
    &ACPI_TABLES
}

/// Parses the system's ACPI tables 
pub fn init(rsdp_address: Option<PhysicalAddress>, page_table: &mut PageTable) -> Result<(), &'static str> {
    // The first step is to search for the RSDP (Root System Descriptor Pointer),
    // which contains the physical address of the RSDT/XSDG (Root/Extended System Descriptor Table).
    let rsdp = rsdp_address
        // This error message will be overwritten by the or_else statement.
        .ok_or("")
        .and_then(|rsdp_address| Rsdp::from_address(rsdp_address, page_table))
        .or_else(|_| Rsdp::get_rsdp(page_table))?;
    let rsdt_phys_addr = rsdp.sdt_address();
    debug!("RXSDT is located in Frame {rsdt_phys_addr:#X}");

    // Now, we get the actual RSDT/XSDT
    {
        let mut acpi_tables = ACPI_TABLES.lock();
        let (sdt_signature, sdt_total_length) = acpi_tables.map_new_table(rsdt_phys_addr, page_table)?;
        acpi_table_handler(&mut acpi_tables, sdt_signature, sdt_total_length, rsdt_phys_addr)?;
    }
    let sdt_addresses: Vec<PhysicalAddress> = {
        let acpi_tables = ACPI_TABLES.lock();
        let rxsdt = rsdt::RsdtXsdt::get(&acpi_tables).ok_or("couldn't get RSDT or XSDT from ACPI tables")?;
        rxsdt.addresses().collect()
    };

    // The RSDT/XSDT tells us where all of the rest of the ACPI tables exist.
    {
        let mut acpi_tables = ACPI_TABLES.lock();
        for sdt_paddr in sdt_addresses {
            // debug!("RXSDT entry: {:#X}", sdt_paddr);
            let (sdt_signature, sdt_total_length) = acpi_tables.map_new_table(sdt_paddr, page_table)?;
            acpi_table_handler(&mut acpi_tables, sdt_signature, sdt_total_length, sdt_paddr)?;
        }
    }

    // FADT is mandatory, and contains the address of the DSDT
    {
        let acpi_tables = ACPI_TABLES.lock();
        let _fadt = fadt::Fadt::get(&acpi_tables).ok_or("The required FADT APIC table wasn't found (signature 'FACP')")?;
        // here: do something with the DSDT here, when needed.
        // debug!("DSDT physical address: {:#X}", {_fadt.dsdt});
    }

    // WAET is optional, and contains info about potentially optimizing timer-related actions.
    {
        let acpi_tables = ACPI_TABLES.lock();
        if let Some(waet) = waet::Waet::get(&acpi_tables) {
            // here: do something with the WAET here, if desired.
            debug!("WAET: RTC? {:?}. ACPI PM timer: {:?}",
                waet.rtc_good(), waet.acpi_pm_timer_good(),
            );
        }
    }
    
    // HPET is optional, but usually present.
    {
        let acpi_tables = ACPI_TABLES.lock();
        if let Some(hpet_table) = hpet::HpetAcpiTable::get(&acpi_tables) {
            let hpet = hpet_table.init_hpet(page_table)?;
            let period = time::Period::new(hpet.read().counter_period_femtoseconds().into());
            time::register_clock_source::<hpet::Hpet>(period);
        } else {
            warn!("This machine has no HPET.");
        }
    };
    
    // MADT is mandatory
    {
        let acpi_tables = ACPI_TABLES.lock();
        let madt = madt::Madt::get(&acpi_tables).ok_or("The required MADT ACPI table wasn't found (signature 'APIC')")?;
        madt.bsp_init(page_table)?;
    }

    // If we have a DMAR table, use it to obtain IOMMU info. 
    {
        let acpi_tables = ACPI_TABLES.lock();
        if let Some(dmar_table) = dmar::Dmar::get(&acpi_tables) {
            debug!("This machine has a DMAR table: flags: {:#b}, host_address_width: {} bits", 
                dmar_table.flags(), dmar_table.host_address_width()
            );

            for table in dmar_table.iter() {
                if let dmar::DmarEntry::Drhd(drhd) = table {
                    debug!("Found DRHD table: INCLUDE_PCI_ALL: {:?}, segment_number: {:#X}, register_base_address: {:#X}", 
                        drhd.include_pci_all(), drhd.segment_number(), drhd.register_base_address(),
                    );
                    if !drhd.include_pci_all() {
                        info!("No IOMMU support when INCLUDE_PCI_ALL not set in DRHD");
                    } else {
                        let register_base_address = PhysicalAddress::new(drhd.register_base_address() as usize)
                            .ok_or("IOMMU register_base_address was invalid")?;
                        iommu::init(
                            dmar_table.host_address_width(),
                            drhd.segment_number(), 
                            register_base_address,
                            page_table
                        )?;
                    }
                    debug!("DRHD table has Device Scope entries:");
                    for (_idx, dev_scope) in drhd.iter().enumerate() {
                        debug!("    Device Scope [{}]: type: {}, enumeration_id: {}, start_bus_number: {}", 
                            _idx, dev_scope.device_type(), dev_scope.enumeration_id(), dev_scope.start_bus_number(),
                        );
                        debug!("                  path: {:?}", dev_scope.path());
                    }
                }
            }
        }
    }

    Ok(())
}
