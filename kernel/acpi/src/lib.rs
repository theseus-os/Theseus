//! Code to parse the ACPI tables, based off of Redox. 
#![no_std]

#![allow(dead_code)] //  to suppress warnings for unused functions/methods
#![allow(unaligned_references)] // temporary, just to suppress unsafe packed borrows 


#[macro_use] extern crate log;
#[macro_use] extern crate lazy_static;
extern crate alloc;
extern crate volatile;
extern crate irq_safety; 
extern crate spin;
extern crate memory;
extern crate kernel_config;
extern crate ioapic;
extern crate pit_clock;
extern crate ap_start;
extern crate pic; 
extern crate apic;
extern crate hpet;
extern crate pause;
extern crate acpi_table;
extern crate acpi_table_handler;
extern crate rsdp;
extern crate rsdt;
extern crate fadt;
extern crate madt;


use alloc::vec::Vec;
use spin::Mutex;
use memory::{PageTable, PhysicalAddress};
use rsdp::Rsdp;
use acpi_table::AcpiTables;
use acpi_table_handler::acpi_table_handler;


lazy_static! {
    /// The singleton instance of the `AcpiTables` struct,
    /// which contains the MappedPages and location of all discovered ACPI tables.
    static ref ACPI_TABLES: Mutex<AcpiTables> = Mutex::new(AcpiTables::default());
}

/// Returns a reference to the singleton instance of all ACPI tables 
/// that have been discovered, mapped, and parsed so far.
pub fn get_acpi_tables() -> &'static Mutex<AcpiTables> {
    &ACPI_TABLES
}

/// Parses the system's ACPI tables 
pub fn init(page_table: &mut PageTable) -> Result<(), &'static str> {
    // The first step is to search for the RSDP (Root System Descriptor Pointer),
    // which contains the physical address of the RSDT/XSDG (Root/Extended System Descriptor Table).
    let rsdp = Rsdp::get_rsdp(page_table)?;
    let rsdt_phys_addr = rsdp.sdt_address();
    debug!("RXSDT is located in Frame {:#X}", rsdt_phys_addr);

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
        for sdt_paddr in sdt_addresses.clone() {
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
        // debug!("DSDT physical address: {:#X}", fadt.dsdt);
    }
    
    // HPET is optional, but usually present.
    {
        let acpi_tables = ACPI_TABLES.lock();
        if let Some(hpet_table) = hpet::HpetAcpiTable::get(&acpi_tables) {
            hpet_table.init_hpet(page_table)?;
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

    Ok(())
}
