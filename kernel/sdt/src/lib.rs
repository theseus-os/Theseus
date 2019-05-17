//! ACPI table definitions and basic SDT structures.

#![no_std]

use core::mem;

/// The size in bytes of the ACPI SDT Header (`Sdt` struct).
pub const SDT_SIZE_IN_BYTES: usize = core::mem::size_of::<Sdt>();

/// An ACPI System Descriptor Table.
/// This is the header (the first part) of every ACPI table.
#[derive(Copy, Clone, Debug)]
#[repr(packed)]
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


/// A struct used to describe the position and layout of registers
/// related to ACPI tables.
#[repr(packed)]
#[derive(Clone, Copy, Debug)]
pub struct GenericAddressStructure {
    pub address_space: u8,
    pub bit_width: u8,
    pub bit_offset: u8,
    pub access_size: u8,
    pub phys_addr: u64,
}

impl Sdt {
    /// Get the address of this tables data
    pub fn data_address(&self) -> usize {
        (self as *const Self as usize) + mem::size_of::<Sdt>()
    }

    /// Get the length of this tables data
    pub fn data_len(&self) -> usize {
        let total_size = self.length as usize;
        let header_size = mem::size_of::<Sdt>();
        if total_size >= header_size {
            total_size - header_size
        } else {
            0
        }
    }

    pub fn matches_pattern(&self, signature: [u8; 4], oem_id: [u8; 6], oem_table_id: [u8; 8]) -> bool{
        self.signature == signature && self.oem_id == oem_id && self.oem_table_id == oem_table_id
    }
}
