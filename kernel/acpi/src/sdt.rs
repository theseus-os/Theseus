use core::mem;
use core::slice;

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

impl Sdt {
    /// Get the address of this tables data
    pub fn data_address(&self) -> usize {
        self as *const _ as usize + mem::size_of::<Sdt>()
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

    pub fn data(&self) -> &[u8] {
        unsafe { slice::from_raw_parts(self.data_address() as *const u8, self.data_len()) }
    }

    pub fn match_pattern(&self, signature: [u8; 4], oem_id: [u8; 6], oem_table_id: [u8; 8]) -> bool{
        self.signature == signature && self.oem_id == oem_id && self.oem_table_id == oem_table_id
    }
}
