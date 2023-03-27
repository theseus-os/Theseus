//! Handles ACPI tables based on signatures.
//! 
//! Essentially a giant multiplexer that invokes the proper
//! table-specific handler function based on an ACPI table signature.

#![no_std]

extern crate alloc;

use log::warn;
use memory::PhysicalAddress;
use acpi_table::{AcpiSignature, AcpiTables};


/// The single arbiter of ACPI Table types, 
/// which contains a large table of ACPI signatures to specific table handler functions. 
/// Each handler invoked by this function will add the specific table
/// to the given list of `AcpiTables`.
/// # Arguments
/// * `acpi_tables`: a mutable reference to the `AcpiTables` that will contain the new table. 
/// * `signature`: the signature of the ACPI table that is being added. 
///    This determines which handler is invoked.
/// * `length`: the total length of the table, which was obtained from its `Sdt` header when originally mapped.
/// * `phys_addr`: the `PhysicalAddress` of the table, which is used for determining where it exists within the mapped region.
pub fn acpi_table_handler(
    acpi_tables: &mut AcpiTables,
    signature: AcpiSignature,
    length: usize,
    phys_addr: PhysicalAddress,
) -> Result<(), &'static str> {

    match &signature {
        // TODO: use a trait to handle this, with an associated const of the table signature 
        //       and an associated type of the root ACPI table for that signature.
        rsdt::RSDT_SIGNATURE |
        rsdt::XSDT_SIGNATURE => rsdt::handle(acpi_tables, signature, length, phys_addr),
        fadt::FADT_SIGNATURE => fadt::handle(acpi_tables, signature, length, phys_addr),
        hpet::HPET_SIGNATURE => hpet::handle(acpi_tables, signature, length, phys_addr),
        madt::MADT_SIGNATURE => madt::handle(acpi_tables, signature, length, phys_addr),
        dmar::DMAR_SIGNATURE => dmar::handle(acpi_tables, signature, length, phys_addr),
        _ => {
            warn!("Skipping unsupported ACPI table {:?}", core::str::from_utf8(&signature).unwrap_or("Unknown Signature"));
            Ok(())
            // Err("Found unsupported ACPI table signature")
        }
    }
}