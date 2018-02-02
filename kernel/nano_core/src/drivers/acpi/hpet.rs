use core::{mem, ptr};
use core::ops::DerefMut;
use core::ptr::{read_volatile, write_volatile};
use spin::Once; 

use memory::{FRAME_ALLOCATOR, Frame, ActivePageTable, PhysicalAddress, Page, VirtualAddress, EntryFlags};

use super::sdt::Sdt;
use super::{ACPI_TABLE, find_sdt, load_table, get_sdt_signature};


static HPET_VIRT_ADDR: Once<VirtualAddress> = Once::new();



#[repr(packed)]
#[derive(Debug)]
pub struct Hpet {
    pub header: Sdt,

    pub hw_rev_id: u8,
    pub comparator_descriptor: u8,
    pub pci_vendor_id: u16,

    pub base_address: GenericAddressStructure,

    pub hpet_number: u8,
    pub min_periodic_clk_tick: u16,
    pub oem_attribute: u8
}

impl Hpet {
    pub fn init(active_table: &mut ActivePageTable) {
        let hpet_sdt = find_sdt("HPET");
        let hpet = if hpet_sdt.len() == 1 {
            load_table(get_sdt_signature(hpet_sdt[0]));
            Hpet::new(hpet_sdt[0], active_table)
        } else {
            error!("Unable to find HPET");
            return;
        };

        if let Some(hpet) = hpet {
            debug!("  HPET: {:X} {:?}", hpet.hpet_number, hpet);

            let mut hpet_t = ACPI_TABLE.hpet.write();
            *hpet_t = Some(hpet);
        }
    }

    pub fn new(sdt: &'static Sdt, active_table: &mut ActivePageTable) -> Option<Hpet> {
        if &sdt.signature == b"HPET" && sdt.length as usize >= mem::size_of::<Hpet>() {
            let s = unsafe { ptr::read((sdt as *const Sdt) as *const Hpet) };
            unsafe { s.base_address.init(active_table) };
            Some(s)
        } else {
            None
        }
    }
}

#[repr(packed)]
#[derive(Clone, Copy, Debug, Default)]
pub struct GenericAddressStructure {
    address_space: u8,
    bit_width: u8,
    bit_offset: u8,
    access_size: u8,
    pub address: u64,
}

impl GenericAddressStructure {
    pub fn init(&self, active_table: &mut ActivePageTable) {
        let vaddr = (self.address + 0xFFFF_FFFF_0000_0000) as VirtualAddress;
        let page = Page::containing_address(vaddr);
        let frame = Frame::containing_address(self.address as PhysicalAddress);
        let mut fa = FRAME_ALLOCATOR.try().unwrap().lock();
        active_table.map_to(page, frame, EntryFlags::PRESENT | EntryFlags::WRITABLE | EntryFlags::NO_EXECUTE, fa.deref_mut());
        HPET_VIRT_ADDR.call_once(|| vaddr);
    }

    pub unsafe fn read_u64(&self, offset: usize) -> u64 {
        read_volatile((self.address as usize + offset) as *const u64)
    }

    pub unsafe fn write_u64(&mut self, offset: usize, value: u64) {
        write_volatile((self.address as usize + offset) as *mut u64, value);
    }
}
