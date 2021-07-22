//! Definitions for the DMAR, the Direct Memory Access (DMA) Remapping ACPI table.
//!
//! The structures defined herein are based on Section 8 of Intel's VT Directed I/O Spec:
//! <https://software.intel.com/content/www/us/en/develop/download/intel-virtualization-technology-for-directed-io-architecture-specification.html>
//!

#![no_std]

extern crate memory;
extern crate sdt;
extern crate acpi_table;
extern crate zerocopy;

use core::mem::size_of;
use memory::{PhysicalAddress, MappedPages};
use sdt::Sdt;
use acpi_table::{AcpiSignature, AcpiTables};
use zerocopy::FromBytes;


pub const DMAR_SIGNATURE: &'static [u8; 4] = b"DMAR";


/// The handler for parsing the DMAR table and adding it to the ACPI tables list.
pub fn handle(
    acpi_tables: &mut AcpiTables,
    signature: AcpiSignature,
    _length: usize,
    phys_addr: PhysicalAddress
) -> Result<(), &'static str> {
    acpi_tables.add_table_location(signature, phys_addr, None)
}


/// The top-level DMAR table, a DMA Remapping Reporting Structure
/// (also called a DMA Remapping Description table).
#[repr(packed)]
#[derive(Clone, Copy, Debug, FromBytes)]
struct DmarReporting {
    header: Sdt,
    host_address_width: u8,
    flags: u8,
    _reserved: [u8; 10],
    // Following this is a variable number of variable-sized DMAR table entries,
    // so we cannot include them here in the static struct definition.
}


/// A wrapper around the DMAR ACPI table ([`DmarReporting`]),
/// which contains details about IOMMU configuration.
/// 
/// You most likely only care about the [`Dmar::iter()`] method.
pub struct Dmar<'t> {
    /// The fixed-size part of the actual DMAR ACPI table.
    table: &'t DmarReporting,
    /// The underlying MappedPages that cover this table
    mapped_pages: &'t MappedPages,
    /// The starting offset of the dynamic part of the DMAR table.
    /// This is to be used as an offset into the above `mapped_pages`.
    dynamic_entries_starting_offset: usize,
    /// The total size in bytes of all dynamic entries.
    /// This is *not* the number of entries.
    dynamic_entries_total_size: usize,
}

impl<'t> Dmar<'t> {
    /// Finds the DMAR in the given `AcpiTables` and returns a reference to it.
    pub fn get(acpi_tables: &'t AcpiTables) -> Option<Dmar<'t> {
        let table: &DmarReporting = acpi_tables.table(&DMAR_SIGNATURE).ok()?;
        let total_length = table.header.length as usize;
        let dynamic_part_length = total_length - size_of::<DmarReporting>();
        let loc = acpi_tables.table_location(&DMAR_SIGNATURE)?;
        Some(Dmar {
            table,
            mapped_pages: acpi_tables.mapping(),
            dynamic_entries_starting_offset: loc.slice_offset_and_length?.0,
            dynamic_entries_total_size: dynamic_part_length,
        })
    }

    /// Returns an iterator over the DMAR's entries,
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
    pub fn flags(&self) -> u32 {
        self.table.flags
    }

    /// Returns the maximum DMA physical addressability (in number of bits) 
    /// supported by this machine.
    pub fn host_address_width(&self) -> u8 {
        // The Host Address Width (HAW) of this machine is computed as (N+1),
        // where N is the value reported in the `host_address_width` field.
        self.host_address_width + 1
    }
}
}


/// An Iterator over the dynamic entries of the [`Dmar`].
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
            let (entry_type, entry_size) = { 
                let entry_record: &EntryRecord = self.mapped_pages.as_type(self.offset).ok()?;
                (entry_record.typ, entry_record.size as usize)
            };
            // Second, use that entry type and size to return the specific DMAR entry struct.
            if (self.offset + entry_size) <= self.end_of_entries {
                let entry: Option<DMAREntry> = match entry_type {
                    ENTRY_TYPE_LOCAL_APIC if entry_size == size_of::<DMARLocalApic>() => {
                        self.mapped_pages.as_type(self.offset).ok().map(|ent| DMAREntry::LocalApic(ent))
                    },
                    ENTRY_TYPE_IO_APIC if entry_size == size_of::<DMARIoApic>() => {
                        self.mapped_pages.as_type(self.offset).ok().map(|ent| DMAREntry::IoApic(ent))
                    },
                    ENTRY_TYPE_INT_SRC_OVERRIDE if entry_size == size_of::<DMARIntSrcOverride>() => {
                        self.mapped_pages.as_type(self.offset).ok().map(|ent| DMAREntry::IntSrcOverride(ent))
                    },
                    ENTRY_TYPE_NON_MASKABLE_INTERRUPT if entry_size == size_of::<DMARNonMaskableInterrupt>() => {
                        self.mapped_pages.as_type(self.offset).ok().map(|ent| DMAREntry::NonMaskableInterrupt(ent))
                    },
                    ENTRY_TYPE_LOCAL_APIC_ADDRESS_OVERRIDE if entry_size == size_of::<DMARLocalApicAddressOverride>() => {
                        self.mapped_pages.as_type(self.offset).ok().map(|ent| DMAREntry::LocalApicAddressOverride(ent))
                    },
                    _ => None,
                };
                // move the offset to the end of this entry, i.e., the beginning of the next entry record
                self.offset += entry_size;
                // return the DMAR entry if properly formed, or if not, return an unknown/corrupt entry.
                entry.or(Some(DMAREntry::UnknownOrCorrupt(entry_type)))
            }
            else {
                None
            }
        }
        else {
            None
        }
    }
}


/// Represents the "header" of each dynamic table entry 
/// in the [`DmarReporting`] table.
#[derive(Clone, Copy, Debug, FromBytes)]
#[repr(packed)]
pub struct DmarEntryRecord {
    /// The type of a DMAR entry.
    /// This should be of type [`DmarEntryTypes`], but it's incompatible with `FromBytes`.
    typ: u16,
    /// The length in bytes of a DMAR entry table.
    length: u16,
}

/// The possible types of entries in the [`DmarReporting`] table.
#[derive(Clone, Copy, Debug)]
#[repr(u16)]
enum DmarEntryTypes {
    Drhd = 0,
    Rmrr = 1,
    Atsr = 2,
    Rhsa = 3, 
    Andd = 4,
    Satc = 5,
    /// Any entry type larger than 5 is reserved for future use.
    Unknown,
}
impl From<u16> for DmarEntryTypes {
    fn from(v: u16) -> Self {
        match v {
            0 => Self::Drhd,
            1 => Self::Rmrr,
            2 => Self::Atsr,
            3 => Self::Rhsa,
            4 => Self::Andd,
            5 => Self::Satc,
            _ => Self::Unknown,
        }
    }
}


/// The set of possible DMAR entries.
#[derive(Copy, Clone, Debug)]
pub enum DmarEntry<'t> {
    Drhd(&'t DmarDrhd),
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
        entry: DmarEntryRecord,
    ) -> Result<DmarEntry<'t>, &'static str> {
        match entry.typ {
            0 => mp.as_type(mp_offset).map(|ent| Self::Drhd(ent)),
            1 => mp.as_type(mp_offset).map(|ent| Self::Rmrr(ent)),
            2 => mp.as_type(mp_offset).map(|ent| Self::Atsr(ent)),
            3 => mp.as_type(mp_offset).map(|ent| Self::Rhsa(ent)),
            4 => mp.as_type(mp_offset).map(|ent| Self::Andd(ent)),
            5 => mp.as_type(mp_offset).map(|ent| Self::Satc(ent)),
            _ => Self::Unknown(entry),
        }
    }
}


/// DRHD: DMAR Hardware Unit Definition Structure.
#[derive(Clone, Copy, Debug, FromBytes)]
#[repr(packed)]
pub struct DmarDrhd {
    header: DmarEntryRecord,
    flags: u8,
    _reserved: u8,
    segment_number: u16,
    register_base_address: u64,
    // Following this is a variable number of variable-sized DMAR device scope table entries,
    // so we cannot include them here in the static struct definition.
}


/// DMAR Device Scope Structure.
///
/// TODO: dealing with this structure is quite complicated. 
///       See Section 8.3.1 of the VT Directed I/O Spec.
#[derive(Clone, Copy, Debug, FromBytes)]
#[repr(packed)]
pub struct DmarDeviceScope {
    typ: u8,
    length: u8,
    _reserved: u16,
    enumeration_id: u8,
    start_bus_number: u8,
    // Following this is a variable-sized `Path` field,
    // so we cannot include it here in the static struct definition.
    // It would look something like:
    // path: [u16],
}


/// RMRR: DMAR Reserved Memory Region Reporting Structure. 
///
/// An instance of this struct describes a memory region
#[derive(Clone, Copy, Debug, FromBytes)]
#[repr(packed)]
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

/// ATSR: DMAR Root Port ATS (Address Translation Services) Capability Reporting Structure. 
#[derive(Clone, Copy, Debug, FromBytes)]
#[repr(packed)]
pub struct DmarAtsr {
    header: DmarEntryRecord,
    flags: u8,
    _reserved: u8,
    segment_number: u16,
    // Following this is a variable number of variable-sized DMAR device scope table entries,
    // so we cannot include them here in the static struct definition.
}

/// RHSA: DMAR Remapping Hardware Static Affinity Structure. 
#[derive(Clone, Copy, Debug, FromBytes)]
#[repr(packed)]
pub struct DmarRhsa {
    header: DmarEntryRecord,
    _reserved: u32,
    register_base_address: u64,
    proximity_domain: u32,
}

/// ANDD: DMAR ACPI Name-space Device Declaration Structure. 
#[derive(Clone, Copy, Debug, FromBytes)]
#[repr(packed)]
pub struct DmarAndd {
    header: DmarEntryRecord,
    _reserved: [u8; 3],
    acpi_device_number: u8,
    // Following this is a variable-sized `ACPI Object Name` field,
    // so we cannot include it here in the static struct definition.
    // It's a C-style null-terminated string, which would look something like:
    // acpi_object_name: [u8],
}

/// SATC: DMAR SoC Integrated Address Translation Cache Reorting Structure. 
#[derive(Clone, Copy, Debug, FromBytes)]
#[repr(packed)]
pub struct DmarSatc {
    header: DmarEntryRecord,
    flags: u8,
    _reserved: u8,
    segment_number: u16,
    // Following this is a variable number of variable-sized DMAR device scope table entries,
    // so we cannot include them here in the static struct definition.
}
