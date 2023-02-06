//! ACPI table definitions and basic SDT structures.
#![no_std]

use zerocopy::FromBytes;

/// The size in bytes of the ACPI SDT Header (`Sdt` struct).
pub const SDT_SIZE_IN_BYTES: usize = core::mem::size_of::<Sdt>();

/// An ACPI System Descriptor Table.
/// This is the header (the first part) of every ACPI table.
#[derive(Copy, Clone, Debug, FromBytes)]
#[repr(C, packed)]
pub struct Sdt {
  pub signature: [u8; 4],
  pub length: u32,
  pub revision: u8,
  pub checksum: u8,
  pub oem_id: [u8; 6],
  pub oem_table_id: [u8; 8],
  pub oem_revision: u32,
  pub creator_id: u32,
  pub creator_revision: u32
}
const _: () = assert!(core::mem::size_of::<Sdt>() == 36);
const _: () = assert!(core::mem::align_of::<Sdt>() == 1);

/// A struct used to describe the position and layout of registers
/// related to ACPI tables.
#[derive(Clone, Copy, Debug, FromBytes)]
#[repr(C, packed)]
pub struct GenericAddressStructure {
    pub address_space: u8,
    pub bit_width: u8,
    pub bit_offset: u8,
    pub access_size: u8,
    pub phys_addr: u64,
}
const _: () = assert!(core::mem::size_of::<GenericAddressStructure>() == 12);
const _: () = assert!(core::mem::align_of::<GenericAddressStructure>() == 1);
