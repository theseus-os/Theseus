//! Definitions for the ACPI RSDT and XSDT system tables.
//!
//! RSDT is the Root System Descriptor Table, whereas
//! XSDT is the Extended System Descriptor Table. 
//! They are identical except that the XSDT uses 64-bit physical addresses
//! to point to other ACPI SDTs, while the RSDT uses 32-bit physical addresses.
//!
//! # Note about alignment
//! Technically the RSDT contains a list of 32-bit addresses (`u32`) and the XSDT has 64-bit addresses (`u64`),
//! but the ACPI tables often aren't aligned to 4-byte and 8-byte addresses. 
//! This lack of alignment causes problems with Rust's slice type, which requires proper alignment.
//! Thus, we store them as slices of individual bytes (`u8`) and calculate the physical addresses
//! on demand when requested in the `RsdtXsdt::addresses()` iterator function.

#![no_std]

use core::mem::size_of;
use memory::PhysicalAddress;
use sdt::{Sdt, SDT_SIZE_IN_BYTES};
use acpi_table::{AcpiSignature, AcpiTables};


pub const RSDT_SIGNATURE: &[u8; 4] = b"RSDT";
pub const XSDT_SIGNATURE: &[u8; 4] = b"XSDT";


/// The handler for parsing RSDT/XSDT tables and adding them to the ACPI tables list.
pub fn handle(
    acpi_tables: &mut AcpiTables,
    signature: AcpiSignature,
    length: usize,
    phys_addr: PhysicalAddress
) -> Result<(), &'static str> {
    // See the crate-level docs for an explanation of why this is always `u8`.
    let slice_element_size = match &signature {
        RSDT_SIGNATURE => size_of::<u8>(),
        XSDT_SIGNATURE => size_of::<u8>(),
        _ => return Err("unexpected ACPI table signature (not RSDT or XSDT)"),
    };

    let slice_paddr = phys_addr + SDT_SIZE_IN_BYTES; // the array of addresses starts right after the SDT header.
    let num_addrs = (length - SDT_SIZE_IN_BYTES) / slice_element_size;
    acpi_tables.add_table_location(signature, phys_addr, Some((slice_paddr, num_addrs)))
}


/// The Root/Extended System Descriptor Table, RSDT or XSDT. 
/// This table primarily contains an array of physical addresses
/// where other ACPI SDTs can be found.
///
/// Use the `addresses()` method to obtain an [`Iterator`] over those physical addresses.
pub struct RsdtXsdt<'t>(RsdtOrXsdt<'t>);
enum RsdtOrXsdt<'t> {
    /// RSDT, which contains 32-bit addresses.
    Regular(Rsdt<'t>),
    /// XSDT, which contains 64-bit addresses.
    Extended(Xsdt<'t>),
}

type Rsdt<'t> = (&'t Sdt, &'t [u8]);
type Xsdt<'t> = (&'t Sdt, &'t [u8]);

impl<'t> RsdtXsdt<'t> {
    /// Finds the RSDT or XSDT in the given `AcpiTables` and returns a reference to it.
    pub fn get(acpi_tables: &'t AcpiTables) -> Option<RsdtXsdt<'t>> {
        if let (Ok(sdt), Ok(addrs)) = (acpi_tables.table::<Sdt>(RSDT_SIGNATURE), acpi_tables.table_slice::<u8>(RSDT_SIGNATURE)) {
            Some(RsdtXsdt(RsdtOrXsdt::Regular((sdt, addrs))))
        }
        else if let (Ok(sdt), Ok(addrs)) = (acpi_tables.table::<Sdt>(XSDT_SIGNATURE), acpi_tables.table_slice::<u8>(XSDT_SIGNATURE)) {
            Some(RsdtXsdt(RsdtOrXsdt::Extended((sdt, addrs))))
        } 
        else {
            None
        }
    }

    /// Returns a reference to the SDT header of this RSDT or XSDT.
    pub fn sdt(&self) -> &Sdt {
        match &self.0 {
            RsdtOrXsdt::Regular(ref r)  => r.0,
            RsdtOrXsdt::Extended(ref x) => x.0,
        }
    }

    /// Returns an [`Iterator`] over the `PhysicalAddress`es of the SDT entries
    /// included in this RSDT or XSDT.
    pub fn addresses(&self) -> impl Iterator<Item = PhysicalAddress> + '_ {
        let mut rsdt_iter = None;
        let mut xsdt_iter = None;
        match &self.0 {
            RsdtOrXsdt::Regular(ref rsdt)  => rsdt_iter = Some(
                rsdt.1.chunks_exact(size_of::<u32>()).map(|bytes| {
                    let arr = [bytes[0], bytes[1], bytes[2], bytes[3]];
                    PhysicalAddress::new_canonical(u32::from_le_bytes(arr) as usize)
                })
            ),
            RsdtOrXsdt::Extended(ref xsdt) => xsdt_iter = Some(
                xsdt.1.chunks_exact(size_of::<u64>()).map(|bytes| {
                    let arr = [bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7]];
                    PhysicalAddress::new_canonical(usize::from_le_bytes(arr))
                })
            ),
        }

        rsdt_iter.into_iter().flatten().chain(xsdt_iter.into_iter().flatten())
    }
}
