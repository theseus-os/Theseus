use core::{mem, ptr};
use core::ops::DerefMut;
use core::ptr::{read_volatile, write_volatile};
use kernel_config::memory::address_page_offset;

use memory::{MappedPages, allocate_pages, FRAME_ALLOCATOR, Frame, ActivePageTable, PhysicalAddress, VirtualAddress, EntryFlags};

use super::sdt::Sdt;
use super::{find_sdt, load_table, get_sdt_signature};


const GENERAL_CAPABILITIES_ID_REG:    usize = 0x0;
const GENERAL_CONFIG_REG:             usize = 0x10;
const GENERAL_INTERRUPT_STATUS_REG:   usize = 0x20;
const MAIN_COUNTER_VALUE_REG:         usize = 0xF0;
const fn timer_config_reg(timer_number: u8) -> usize {
    0x100 + (0x20 * timer_number as usize)
}
const fn timer_comparator_reg(timer_number: u8) -> usize {
    0x108 + (0x20 * timer_number as usize)
}
const fn timer_fsb_interrupt_rout_reg(timer_number: u8) -> usize {
    0x110 + (0x20 * timer_number as usize)
}



pub struct Hpet {
    inner: HpetInner,
    virt_addr: VirtualAddress,
    page: MappedPages,
}

impl Hpet {
    /// Finds and initializes the HPET, and enables its main counter.
    pub fn init(active_table: &mut ActivePageTable) -> Result<Hpet, &'static str> {
        let hpet_sdt = find_sdt("HPET");
        let hpet_inner = try!( 
            if hpet_sdt.len() == 1 {
                load_table(get_sdt_signature(hpet_sdt[0]));
                HpetInner::new(hpet_sdt[0])
            } else {
                Err("unable to find HPET SDT")
            }
        );

        let (hpet_page, vaddr) = try!(hpet_inner.base_address.map_hpet(active_table));
        debug!("HPET: vaddr: {:#X}, hpet_num: {:#X}, HpetInner {:?}", vaddr, hpet_inner.hpet_number, hpet_inner);
        let mut hpet = Hpet {
            inner: hpet_inner,
            virt_addr: vaddr,
            page: hpet_page,
        };

        hpet.enable_counter(true);

        debug!("HPET period: {}, counter val: {}, num timers: {}", 
                hpet.counter_period_femtoseconds(), hpet.get_counter(), hpet.num_timers()
        );

        Ok(hpet)
    }

    /// Returns the HPET's main counter value
    pub fn get_counter(&self) -> u64 {
        unsafe { self.read_u64(MAIN_COUNTER_VALUE_REG) }
    }

    /// Turns on or off the main counter
    pub fn enable_counter(&mut self, enable: bool) {
        unsafe { 
            let old_val = self.read_u64(GENERAL_CONFIG_REG);
            let flag = if enable { 0x1 } else { 0x0 };
            self.write_u64(GENERAL_CONFIG_REG, old_val | flag); 
        }
    }

    /// Must not be zero, must be less or equal to 0x05F5E100 (100 nanoseconds)
    pub fn counter_period_femtoseconds(&self) -> u32 {
        let caps = self.general_capabilities_register();
        let period = caps >> 32;
        period as u32
    }

    pub fn vendor_id(&self) -> u16 {
        let caps = self.general_capabilities_register();
        let id = caps >> 16;
        id as u16
    }

    pub fn num_timers(&self) -> u8 {
        let caps = self.general_capabilities_register();
        // only bits [12:8] matter
        let count = (caps >> 8) & 0b11111; // only 5 bits matter
        // that gives us the number of timers minus one, so add one back to it
        (count + 1) as u8
    }

    fn general_capabilities_register(&self) -> u64 {
        unsafe { self.read_u64(GENERAL_CAPABILITIES_ID_REG) }
    }

    unsafe fn read_u64(&self, offset: usize) -> u64 {
        read_volatile((self.virt_addr as usize + offset) as *const u64)
    }

    unsafe fn write_u64(&mut self, offset: usize, value: u64) {
        write_volatile((self.virt_addr as usize + offset) as *mut u64, value);
    }
}


#[repr(packed)]
#[derive(Debug)]
pub struct HpetInner {
    pub header: Sdt,

    pub hw_rev_id: u8,
    pub comparator_descriptor: u8,
    pub pci_vendor_id: u16,

    pub base_address: GenericAddressStructure,

    pub hpet_number: u8,
    pub min_periodic_clk_tick: u16,
    pub oem_attribute: u8
}

impl HpetInner {
    pub fn new(sdt: &'static Sdt) -> Result<HpetInner, &'static str> {
        if &sdt.signature == b"HPET" && sdt.length as usize >= mem::size_of::<HpetInner>() {
            let hi = unsafe { 
                ptr::read((sdt as *const Sdt) as *const HpetInner) 
            };
            Ok(hi)
        } else {
            Err("Couldn't create new HpetInner SDT")
        }
    }
}

#[repr(packed)]
#[derive(Debug)]
// #[derive(Clone, Copy, Default)]
pub struct GenericAddressStructure {
    _address_space: u8,
    _bit_width: u8,
    _bit_offset: u8,
    _access_size: u8,
    pub address: u64,
}

impl GenericAddressStructure {
    /// Returns a tuple of (HPET MappedPages, virt_addr).
    fn map_hpet(&self, active_table: &mut ActivePageTable) -> Result<(MappedPages, usize), &'static str> {
        let page = try!(allocate_pages(1).ok_or("Couldn't allocate_pages one page")); // only need one page for HPET data
        let frame = Frame::range_inclusive_addr(self.address as PhysicalAddress, 1); // 1 byte long, we just want 1 page
        let mut fa = try!(FRAME_ALLOCATOR.try().ok_or("Couldn't get Frame allocator")).lock();
        let hpet_page = try!(active_table.map_allocated_pages_to(page, frame, 
            EntryFlags::PRESENT | EntryFlags::WRITABLE | EntryFlags::NO_CACHE | EntryFlags::NO_EXECUTE, fa.deref_mut())
        );
        let vaddr = hpet_page.start_address() + address_page_offset(self.address as usize);
        Ok((hpet_page, vaddr))
    }
}
