use super::Revision;

/// All standard UEFI tables begin with a common header.
#[derive(Debug)]
#[repr(C)]
pub struct Header {
    /// Unique identifier for this table.
    pub signature: u64,
    /// Revision of the spec this table conforms to.
    pub revision: Revision,
    /// The size in bytes of the entire table.
    pub size: u32,
    /// 32-bit CRC-32-Castagnoli of the entire table,
    /// calculated with this field set to 0.
    pub crc: u32,
    /// Reserved field that must be set to 0.
    _reserved: u32,
}
