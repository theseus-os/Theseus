//! Definitions for the DMAR, the Direct Memory Access (DMA) Remapping ACPI table.
//!
//! The structures defined herein are based on Section 8 of Intel's VT Directed I/O Spec:
//! <https://software.intel.com/content/www/us/en/develop/download/intel-virtualization-technology-for-directed-io-architecture-specification.html>
//!

#![no_std]

use core::mem::size_of;
use memory::{PhysicalAddress, MappedPages};
use sdt::Sdt;
use acpi_table::{AcpiSignature, AcpiTables};
use zerocopy::FromBytes;

mod drhd;
mod device_scope;

// TODO: once these sub-tables are complete, uncomment them.
// mod rmrr;
// mod atsr;
// mod rhsa;
// mod andd;
// mod satc;

pub use drhd::*;
pub use device_scope::*;
// pub use rmrr::*;
// pub use atsr::*;
// pub use rhsa::*;
// pub use andd::*;
// pub use satc::*;


pub const DMAR_SIGNATURE: &[u8; 4] = b"DMAR";


/// The handler for parsing the DMAR table and adding it to the ACPI tables list.
pub fn handle(
    acpi_tables: &mut AcpiTables,
    signature: AcpiSignature,
    _length: usize,
    phys_addr: PhysicalAddress
) -> Result<(), &'static str> {
    // The DMAR has a variable number of entries, and each entry is of variable size. 
    // So we can't determine the slice_length (just use 0 instead), but we can determine where it starts.
    let slice_start_paddr = phys_addr + size_of::<DmarReporting>();
    acpi_tables.add_table_location(signature, phys_addr, Some((slice_start_paddr, 0)))
}


/// The top-level DMAR table, a DMA Remapping Reporting Structure
/// (also called a DMA Remapping Description table).
///
/// This table is described in Section 8.1 of the VT Directed I/O Spec.
#[derive(Clone, Copy, Debug, FromBytes)]
#[repr(C, packed)]
struct DmarReporting {
    header: Sdt,
    host_address_width: u8,
    flags: u8,
    _reserved: [u8; 10],
    // Following this is a variable number of variable-sized DMAR table entries,
    // so we cannot include them here in the static struct definition.
}
const _: () = assert!(core::mem::size_of::<DmarReporting>() == 48);
const _: () = assert!(core::mem::align_of::<DmarReporting>() == 1);


/// A wrapper around the DMAR ACPI table ([`DmarReporting`]),
/// which contains details about IOMMU configuration.
/// 
/// You most likely care about the [`Dmar::iter()`]
/// and [`Dmar::host_address_width()`] methods.
#[derive(Debug)]
pub struct Dmar<'t> {
    /// The fixed-size part of the actual DMAR ACPI table.
    table: &'t DmarReporting,
    /// The underlying MappedPages that cover this table
    mapped_pages: &'t MappedPages,
    /// The offset into the above `mapped_pages` at which the dynamic part
    /// of the DMAR table begins.
    dynamic_entries_starting_offset: usize,
    /// The total size in bytes of all dynamic entries.
    /// This is *not* the number of entries.
    dynamic_entries_total_size: usize,
}

impl<'t> Dmar<'t> {
    /// Finds the DMAR in the given `AcpiTables` and returns a reference to it.
    pub fn get(acpi_tables: &'t AcpiTables) -> Option<Dmar<'t>> {
        let table: &DmarReporting = acpi_tables.table(DMAR_SIGNATURE).ok()?;
        let total_length = table.header.length as usize;
        let dynamic_part_length = total_length - size_of::<DmarReporting>();
        let loc = acpi_tables.table_location(DMAR_SIGNATURE)?;
        Some(Dmar {
            table,
            mapped_pages: acpi_tables.mapping(),
            dynamic_entries_starting_offset: loc.slice_offset_and_length?.0,
            dynamic_entries_total_size: dynamic_part_length,
        })
    }

    /// Returns an [`Iterator`] over the DMAR's remapping structures ([`DmarEntry`]s),
    /// which are variable in both number and size.
    pub fn iter(&self) -> DmarIter {
        DmarIter {
            mapped_pages: self.mapped_pages,
            offset: self.dynamic_entries_starting_offset,
            end_of_entries: self.dynamic_entries_starting_offset + self.dynamic_entries_total_size,
        }
    }

    /// Returns a reference to the `Sdt` header in this DMAR table.
    pub fn sdt(&self) -> &Sdt {
        &self.table.header
    }

    /// Returns the `flags` value in this DMAR table.
    pub fn flags(&self) -> u8 {
        self.table.flags
    }

    /// Returns the maximum DMA physical addressability (in number of bits) 
    /// supported by this machine.
    pub fn host_address_width(&self) -> u8 {
        // The Host Address Width (HAW) of this machine is computed as (N+1),
        // where N is the value reported in the `host_address_width` field.
        self.table.host_address_width + 1
    }
}


/// An [`Iterator`] over the dynamic entries of the [`Dmar`].
/// Its lifetime is dependent upon the lifetime of its [`Dmar`] instance,
/// which itself is bound to the lifetime of the underlying [`AcpiTables`]. 
#[derive(Clone)]
pub struct DmarIter<'t> {
    /// The underlying MappedPages that contain all ACPI tables.
    mapped_pages: &'t MappedPages,
    /// The offset of the next entry, which should point to a [`DmarEntryRecord`]
    /// at the start of each iteration.
    offset: usize,
    /// The end bound of all DMAR entries. 
    /// This is fixed and should not ever change throughout iteration.
    end_of_entries: usize,
}

impl<'t> Iterator for DmarIter<'t> {
    type Item = DmarEntry<'t>;

    fn next(&mut self) -> Option<Self::Item> {
        if (self.offset + size_of::<DmarEntryRecord>()) < self.end_of_entries {
            // First, we get the next entry record to get the type and size of the actual entry.
            let entry: &DmarEntryRecord = self.mapped_pages.as_type(self.offset).ok()?;
            // Second, use that entry record to return the specific DMAR entry struct.
            if (self.offset + entry.length as usize) <= self.end_of_entries {
                let table = DmarEntry::from_entry(self.mapped_pages, self.offset, entry);
                // move the offset to the end of this entry, i.e., the beginning of the next entry record
                self.offset += entry.length as usize;
                return table.ok();
            }
        }
        None
    }
}


/// Represents the "header" of each dynamic table entry 
/// in the [`DmarReporting`] table.
#[derive(Clone, Copy, Debug, FromBytes)]
#[repr(C, packed)]
pub struct DmarEntryRecord {
    /// The type of a DMAR entry.
    typ: u16,
    /// The length in bytes of a DMAR entry table.
    length: u16,
}
const _: () = assert!(core::mem::size_of::<DmarEntryRecord>() == 4);
const _: () = assert!(core::mem::align_of::<DmarEntryRecord>() == 1);


/// The set of possible sub-tables that can exist in the top-level DMAR table.
///
/// The types of sub-tables are described in Section 8.2 of the VT Directed I/O Spec.
#[derive(Debug)]
pub enum DmarEntry<'t> {
    Drhd(DmarDrhd<'t>),
    Rmrr(&'t DmarRmrr),
    Atsr(&'t DmarAtsr),
    Rhsa(&'t DmarRhsa),
    Andd(&'t DmarAndd),
    Satc(&'t DmarSatc),
    /// The DMAR table had an entry of an unknown type or mismatched length,
    /// so the table entry was malformed and unusable.
    /// The entry type ID is included.
    UnknownOrCorrupt(DmarEntryRecord)
}
impl<'t> DmarEntry<'t> {
    fn from_entry(
        mp: &'t MappedPages,
        mp_offset: usize,
        entry: &DmarEntryRecord,
    ) -> Result<DmarEntry<'t>, &'static str> {
        if entry.typ != 0 {
            log::warn!("Note: non-DRHD remapping structure types (1, 2, 3, 4, or 5) are unimplemented!");
        }
        match entry.typ {
            0 => Ok(Self::Drhd(DmarDrhd::from_entry(mp, mp_offset, entry)?)),
            1 => mp.as_type(mp_offset).map(Self::Rmrr),
            2 => mp.as_type(mp_offset).map(Self::Atsr),
            3 => mp.as_type(mp_offset).map(Self::Rhsa),
            4 => mp.as_type(mp_offset).map(Self::Andd),
            5 => mp.as_type(mp_offset).map(Self::Satc),
            _ => Ok(Self::UnknownOrCorrupt(*entry)),
        }
    }
}


/// RMRR: DMAR Reserved Memory Region Reporting Structure. 
///
/// An instance of this struct describes a memory region
#[derive(Clone, Copy, Debug, FromBytes)]
#[repr(C, packed)]
#[allow(dead_code)]
pub struct DmarRmrr {
    header: DmarEntryRecord,
    _reserved: u16,
    segment_number: u16,
    /// The base address of a 4KB-aligned reserved memory region. 
    base_address: u64,
    /// The upper limit (last address) of the reserved memory region. 
    limit_address: u64,
    // Following this is a variable number of variable-sized DMAR device scope table entries,
    // so we cannot include them here in the static struct definition.
}
const _: () = assert!(core::mem::size_of::<DmarRmrr>() == 24);
const _: () = assert!(core::mem::align_of::<DmarRmrr>() == 1);


/// ATSR: DMAR Root Port ATS (Address Translation Services) Capability Reporting Structure. 
#[derive(Clone, Copy, Debug, FromBytes)]
#[repr(C, packed)]
#[allow(dead_code)]
pub struct DmarAtsr {
    header: DmarEntryRecord,
    flags: u8,
    _reserved: u8,
    segment_number: u16,
    // Following this is a variable number of variable-sized DMAR device scope table entries,
    // so we cannot include them here in the static struct definition.
}
const _: () = assert!(core::mem::size_of::<DmarAtsr>() == 8);
const _: () = assert!(core::mem::align_of::<DmarAtsr>() == 1);


/// RHSA: DMAR Remapping Hardware Static Affinity Structure. 
#[derive(Clone, Copy, Debug, FromBytes)]
#[repr(C, packed)]
#[allow(dead_code)]
pub struct DmarRhsa {
    header: DmarEntryRecord,
    _reserved: u32,
    register_base_address: u64,
    proximity_domain: u32,
}
const _: () = assert!(core::mem::size_of::<DmarRhsa>() == 20);
const _: () = assert!(core::mem::align_of::<DmarRhsa>() == 1);


/// ANDD: DMAR ACPI Name-space Device Declaration Structure. 
#[derive(Clone, Copy, Debug, FromBytes)]
#[repr(C, packed)]
#[allow(dead_code)]
pub struct DmarAndd {
    header: DmarEntryRecord,
    _reserved: [u8; 3],
    acpi_device_number: u8,
    // Following this is a variable-sized `ACPI Object Name` field,
    // so we cannot include it here in the static struct definition.
    // It's a C-style null-terminated string, which would look something like:
    // acpi_object_name: [u8],
}
const _: () = assert!(core::mem::size_of::<DmarAndd>() == 8);
const _: () = assert!(core::mem::align_of::<DmarAndd>() == 1);


/// SATC: DMAR SoC Integrated Address Translation Cache Reorting Structure. 
#[derive(Clone, Copy, Debug, FromBytes)]
#[repr(C, packed)]
#[allow(dead_code)]
pub struct DmarSatc {
    header: DmarEntryRecord,
    flags: u8,
    _reserved: u8,
    segment_number: u16,
    // Following this is a variable number of variable-sized DMAR device scope table entries,
    // so we cannot include them here in the static struct definition.
}
const _: () = assert!(core::mem::size_of::<DmarSatc>() == 8);
const _: () = assert!(core::mem::align_of::<DmarSatc>() == 1);
