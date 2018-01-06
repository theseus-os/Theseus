use core::mem;
use alloc::boxed::Box;

use super::sdt::Sdt;
use super::rxsdt::Rxsdt;

#[derive(Debug)]
pub struct Rsdt(&'static Sdt);

impl Rsdt {
    pub fn new(sdt: &'static Sdt) -> Option<Rsdt> {
        if &sdt.signature == b"RSDT" {
            Some(Rsdt(sdt))
        } else {
            None
        }
    }
}

impl Rxsdt for Rsdt {
    fn iter(&self) -> Box<Iterator<Item = usize>> {
        Box::new(RsdtIter {
            sdt: self.0,
            i: 0
        })
    }
}

pub struct RsdtIter {
    sdt: &'static Sdt,
    i: usize
}

impl Iterator for RsdtIter {
    type Item = usize;
    fn next(&mut self) -> Option<Self::Item> {
        if self.i < self.sdt.data_len()/mem::size_of::<u32>() {
            let item = unsafe { *(self.sdt.data_address() as *const u32).offset(self.i as isize) };
            self.i += 1;
            Some(item as usize)
        } else {
            None
        }
    }
}
