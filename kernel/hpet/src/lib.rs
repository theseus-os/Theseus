//! Support for the x86 HPET: High Precision Event Timer.

#![no_std]

extern crate alloc;
#[macro_use] extern crate log;
extern crate kernel_config;
extern crate memory;
extern crate volatile;
extern crate sdt;
extern crate spin;
extern crate owning_ref;

use core::{mem, ptr};
use core::ops::DerefMut;
use volatile::{Volatile, ReadOnly};
use owning_ref::BoxRefMut;
use alloc::boxed::Box;
use spin::{RwLock, RwLockReadGuard, RwLockWriteGuard};
use memory::{MappedPages, allocate_pages, FRAME_ALLOCATOR, Frame, PageTable, PhysicalAddress, EntryFlags};
use sdt::Sdt;

/// The static instance of the HPET's ACPI memory region, which derefs to an Hpet instance.
static HPET: RwLock<Option<BoxRefMut<MappedPages, Hpet>>> = RwLock::new(None);


/// Returns a reference to the HPET timer structure, wrapped in an Option,
/// because it is not guaranteed that HPET exists or has been initialized.
/// # Example
/// ```
/// let counter_val = get_hpet().as_ref().unwrap().get_counter();
/// ```
pub fn get_hpet() -> RwLockReadGuard<'static, Option<BoxRefMut<MappedPages, Hpet>>> {
    HPET.read()
}

/// Returns a mutable reference to the HPET timer structure, wrapped in an Option,
/// because it is not guaranteed that HPET exists or has been initialized.
/// # Example
/// ```
/// get_hpet_mut().as_mut().unwrap().enable_counter(true);
/// ```
pub fn get_hpet_mut() -> RwLockWriteGuard<'static, Option<BoxRefMut<MappedPages, Hpet>>> {
    HPET.write()
}


/// A structure that offers access to HPET through its I/O registers, 
/// specified by the format here: <https://wiki.osdev.org/HPET#HPET_registers>.
#[repr(C)]
pub struct Hpet {
    /// The General Capabilities and ID Register, at offset 0x0.
    pub general_capabilities_and_id: ReadOnly<u64>,
    _padding0:                       u64,
    /// The General Configuration Register, at offset 0x10.
    pub general_configuration:       Volatile<u64>,
    _padding1:                       u64,
    /// The General Interrupt Status Register, at offset 0x20.
    pub general_interrupt_status:    Volatile<u64>,
    _padding2:                       [u64; (0xF0 - 0x28) / 8], // 25 u64s
    /// The Main Counter Value Register, at offset 0xF0.
    pub main_counter_value:          Volatile<u64>,
    _padding3:                       u64,
    /// The timers (comparators) available for separate.
    /// There is a minimum of 3 timers and a maximum of 32 in an HPET-enabled system.
    /// Call [`num_timers`](#method.num_timers) to get the actual number of HPET timers.
    pub timers:                      [HpetTimer; 32],
}

impl Hpet {
    /// Returns the HPET's main counter value
    pub fn get_counter(&self) -> u64 {
        self.main_counter_value.read()
    }

    /// Turns on or off the main counter
    pub fn enable_counter(&mut self, enable: bool) {
        if enable {
            // set bit 0
            self.general_configuration.update(|old_val_ref| *old_val_ref |= 0x1);
        }
        else {
            // clear bit 0
            self.general_configuration.update(|old_val_ref| *old_val_ref &= !0x1);
        }
            
    }

    /// Returns the period of the HPET counter in femtoseconds,
    /// i.e., the length of time that one HPET tick takes.
    /// 
    /// Can be used to calculate the frequency of the HPET clock.
    /// 
    /// Must not be zero, must be less or equal to 0x05F5E100 (100 nanoseconds)
    pub fn counter_period_femtoseconds(&self) -> u32 {
        let caps = self.general_capabilities_and_id.read();
        let period = caps >> 32;
        period as u32
    }

    pub fn vendor_id(&self) -> u16 {
        let caps = self.general_capabilities_and_id.read();
        let id = caps >> 16;
        id as u16
    }

    pub fn num_timers(&self) -> u8 {
        let caps = self.general_capabilities_and_id.read();
        // only bits [12:8] matter
        let count = (caps >> 8) & 0b11111; // only 5 bits matter
        // that gives us the number of timers minus one, so add one back to it
        (count + 1) as u8
    }
}


/// A structure that wraps HPET I/O register for each timer comparator, 
/// specified by the format here: <https://wiki.osdev.org/HPET#HPET_registers>.
/// There are between 3 and 32 of these in an HPET-enabled system.
#[repr(packed)]
pub struct HpetTimer {
    /// This timer's Configuration and Capability register.
    pub configuration_and_capability: Volatile<u64>,
    /// This timer's Comparator Value register.
    pub comparator_value:             Volatile<u64>,
    /// This timer's FSB Interrupt Route register.
    /// Some info here: <https://wiki.osdev.org/HPET#FSB_mapping>
    pub fsb_interrupt_route:          Volatile<u64>,
    _padding:                         u64,
}


/// Finds and initializes the HPET, and enables its main counter.
/// Returns a mutable reference to the `Hpet` struct
pub fn init(hpet_sdt: &'static Sdt, page_table: &mut PageTable) -> Result<(), &'static str> {
    let hpet_inner = HpetInner::new(hpet_sdt)?;    

    let phys_addr = PhysicalAddress::new(hpet_inner.gen_addr_struct.address as usize)?;
    let page = try!(allocate_pages(1).ok_or("Couldn't allocate_pages one page")); // only need one page for HPET data
    let frame = Frame::range_inclusive_addr(phys_addr, 1);  // 1 byte long, we just want 1 page
    let mut fa = try!(FRAME_ALLOCATOR.try().ok_or("Couldn't get Frame allocator")).lock();
    let hpet_page = try!(page_table.map_allocated_pages_to(page, frame, 
        EntryFlags::PRESENT | EntryFlags::WRITABLE | EntryFlags::NO_CACHE | EntryFlags::NO_EXECUTE, fa.deref_mut())
    );

    let mut hpet = BoxRefMut::new(Box::new(hpet_page)).try_map_mut(|mp| mp.as_type_mut::<Hpet>(phys_addr.frame_offset()))?;
    // get an HPET instance here just to initially enable the main counter
    {
        hpet.enable_counter(true);
        debug!("Initialized HPET, period: {}, counter val: {}, num timers: {}, vendor_id: {}", 
            hpet.counter_period_femtoseconds(), hpet.get_counter(), hpet.num_timers(), hpet.vendor_id()
        );
    }

    *HPET.write() = Some(hpet);

    Ok(())
}


#[repr(packed)]
struct HpetInner {
    _header: Sdt,

    _hw_rev_id: u8,
    _comparator_descriptor: u8,
    _pci_vendor_id: u16,
    gen_addr_struct: GenericAddressStructure,
    _hpet_number: u8,
    _min_periodic_clk_tick: u16,
    _oem_attribute: u8
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
struct GenericAddressStructure {
    _address_space: u8,
    _bit_width: u8,
    _bit_offset: u8,
    _access_size: u8,
    /// We only care about this field, the physical address of the structure.
    address: u64,
}

