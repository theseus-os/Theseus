//! Definitions for the ACPI RSDT and XSDT system tables.
//!
//! RSDT is the Root System Descriptor Table, whereas
//! XSDT is the Extended System Descriptor Table. 
//! They are identical except that the XSDT uses 64-bit physical addresses
//! to point to other ACPI SDTs, while the RSDT uses 32-bit physical addresses.

#![no_std]

extern crate memory;
extern crate sdt;
extern crate acpi_table;

use core::mem::size_of;
use memory::PhysicalAddress;
use sdt::{Sdt, SDT_SIZE_IN_BYTES};
use acpi_table::{AcpiSignature, AcpiTables};


pub const RSDT_SIGNATURE: &'static [u8; 4] = b"RSDT";
pub const XSDT_SIGNATURE: &'static [u8; 4] = b"XSDT";


/// The handler for parsing RSDT/XSDT tables and adding them to the ACPI tables list.
pub fn handle(
    acpi_tables: &mut AcpiTables,
    signature: AcpiSignature,
    length: usize,
    phys_addr: PhysicalAddress
) -> Result<(), &'static str> {
    let slice_element_size = match &signature {
        RSDT_SIGNATURE => size_of::<u32>(),
        XSDT_SIGNATURE => size_of::<u64>(),
        _ => return Err("unexpected ACPI table signature (not RSDT or XSDT)"),
    };

    let slice_paddr = phys_addr + SDT_SIZE_IN_BYTES; // the array of addresses starts right after the SDT header.
    let num_addrs = (length - SDT_SIZE_IN_BYTES) / slice_element_size;
    acpi_tables.add_table_location(signature, phys_addr, Some((slice_paddr, num_addrs)))
}


/// The Root/Extended System Descriptor Table,
/// which primarily contains an array of physical addresses
/// (32-bit if the regular RSDT, 64-bit if the extended XSDT)
/// where other ACPI SDTs can be found.
pub struct RsdtXsdt<'t>(RsdtOrXsdt<'t>);

impl<'t> RsdtXsdt<'t> {
    /// Finds the RSDT or XSDT in the given `AcpiTables` and returns a reference to it.
    pub fn get(acpi_tables: &'t AcpiTables) -> Option<RsdtXsdt<'t>> {
        if let (Ok(sdt), Ok(addrs)) = (acpi_tables.table::<Sdt>(&RSDT_SIGNATURE), acpi_tables.table_slice::<u32>(&RSDT_SIGNATURE)) {
            Some(RsdtXsdt(RsdtOrXsdt::Regular((sdt, addrs))))
        }
        else if let (Ok(sdt), Ok(addrs)) = (acpi_tables.table::<Sdt>(&XSDT_SIGNATURE), acpi_tables.table_slice::<u64>(&XSDT_SIGNATURE)) {
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

    /// Returns an iterator over the `PhysicalAddress`es of the SDT entries
    /// included in this RSDT or XSDT.
    pub fn addresses<'r>(&'r self) -> impl Iterator<Item = PhysicalAddress> + 'r {
        // Ideally, we would do something like this, but Rust doesn't allow match arms to have different types (iterator map closures are types...)
        // match &self.0 {
        //     RsdtOrXsdt::Regular(ref r)  => r.1.iter().map(|paddr| PhysicalAddress::new_canonical(*paddr as usize)),
        //     RsdtOrXsdt::Extended(ref x) => x.1.iter().map(|paddr| PhysicalAddress::new_canonical(*paddr as usize)),
        // }
        //
        // So, instead, we use a little trick inspired by this post:
        // https://stackoverflow.com/questions/29760668/conditionally-iterate-over-one-of-several-possible-iterators

        let r_iter = if let RsdtOrXsdt::Regular(ref r) = self.0 {
            Some(r.1.iter().map(|paddr| PhysicalAddress::new_canonical(*paddr as usize)))
        } else {
            None
        };
        let x_iter = if let RsdtOrXsdt::Extended(ref x) = self.0 {
            Some(x.1.iter().map(|paddr| PhysicalAddress::new_canonical(*paddr as usize)))
        } else {
            None
        };
        r_iter.into_iter().flatten().chain(x_iter.into_iter().flatten())
    }
}


/// Either an RSDT or an XSDT. 
/// The RSDT specifies that there are a variable number of 
/// 32-bit physical addresses following the SDT header,
/// while the XSDT is the same but with 64-bit physical addresses.
enum RsdtOrXsdt<'t> {
    /// RSDT
    Regular(Rsdt<'t>),
    /// XSDT
    Extended(Xsdt<'t>),
}

type Rsdt<'t> = (&'t Sdt, &'t [u32]);
type Xsdt<'t> = (&'t Sdt, &'t [u64]);
