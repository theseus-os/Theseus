use alloc::boxed::Box;

use memory::ActivePageTable;

use super::sdt::Sdt;
use super::get_sdt;

pub trait Rxsdt {
    fn iter(&self) -> Box<Iterator<Item = usize>>;

    fn map_all(&self, active_table: &mut ActivePageTable) -> Result<(), &'static str> {
        for sdt_paddr in self.iter() {
            try!(get_sdt(sdt_paddr, active_table));
        }
        Ok(())
    }

    fn find(&self, signature: [u8; 4], oem_id: [u8; 6], oem_table_id: [u8; 8]) -> Option<&'static Sdt> {
        for sdt in self.iter() {
            let sdt = unsafe { &*(sdt as *const Sdt) };

            if sdt.match_pattern(signature, oem_id, oem_table_id) {
                return Some(sdt);
            }
        }

        None
    }

    fn length(&self) -> usize;
}
