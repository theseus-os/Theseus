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

use memory::PhysicalAddress;
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
pub struct DmarReporting {
    pub header: Sdt,
    host_address_width: u8,
    pub flags: u8,
    _reserved: [u8; 10],
    // Following this is a variable number of variable-sized DMAR table entries,
    // so we cannot include them here in the static struct definition.
}

impl DmarReporting {
    /// Finds the top-level DMAR table in the given `AcpiTables` and returns a reference to it.
    pub fn get<'t>(acpi_tables: &'t AcpiTables) -> Option<&'t DmarReporting> {
        acpi_tables.table(&DMAR_SIGNATURE).ok()
    }

    /// Returns the maximum DMA physical addressability (in number of bits) 
    /// supported by this machine.
    pub fn host_address_width(&self) -> u8 {
        // The Host Address Width (HAW) of this machine is computed as (N+1),
        // where N is the value reported in the `host_address_width` field.
        self.host_address_width + 1
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


/// DRHD: DMAR Hardware Unit Definition Structure.
#[derive(Clone, Copy, Debug, FromBytes)]
#[repr(packed)]
pub struct DmarHardwareUnitDefinition {
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
pub struct DmarReservedMemoryRegionReporting {
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
