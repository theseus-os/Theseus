//! Code to parse the ACPI tables, based off of Redox. 
#![no_std]
#![feature(alloc)]
#![feature(const_fn)]

#![allow(dead_code)] //  to suppress warnings for unused functions/methods
#![allow(safe_packed_borrows)] // temporary, just to suppress unsafe packed borrows 


#[macro_use] extern crate log;
#[macro_use] extern crate lazy_static;
#[macro_use] extern crate alloc;
extern crate volatile;
extern crate owning_ref;
extern crate irq_safety; 
extern crate spin;
extern crate memory;
extern crate kernel_config;
extern crate ioapic;
extern crate pit_clock;
extern crate ap_start;
extern crate pic; 
extern crate apic;


macro_rules! try_opt {
    ($e:expr) =>(
        match $e {
            Some(v) => v,
            None => return None,
        }
    )
}



use alloc::btree_map::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use alloc::boxed::Box;
use core::ops::DerefMut;
use owning_ref::BoxRefMut;
use spin::{Mutex, RwLock, RwLockReadGuard, RwLockWriteGuard};


use memory::{ActivePageTable, allocate_pages, MappedPages, PhysicalMemoryArea, VirtualAddress, PhysicalAddress, Frame, EntryFlags, FRAME_ALLOCATOR};
use kernel_config::memory::{PAGE_SIZE, address_page_offset};

// pub use self::dmar::Dmar;
pub use self::fadt::Fadt;
pub use self::madt::Madt;
pub use self::rsdt::Rsdt;
pub use self::sdt::Sdt;
pub use self::xsdt::Xsdt;
pub use self::rxsdt::Rxsdt;
pub use self::rsdp::RSDP;

use self::hpet::Hpet;
// use self::aml::{parse_aml_table, AmlError, AmlValue};

// mod dmar;
// mod aml;
pub mod hpet;
mod fadt;
pub mod madt;
mod rsdt;
mod sdt;
mod xsdt;
mod rxsdt;
mod rsdp;


/// The address that an AP jumps to when it first is booted by the BSP
/// For x2apic systems, this must be at 0x10000 or higher! 
const AP_STARTUP: PhysicalAddress = 0x10000; 
/// small 512-byte area for AP startup data passed from the BSP in long mode (Rust) code.
/// Value: 0xF000
const TRAMPOLINE: PhysicalAddress = AP_STARTUP - PAGE_SIZE;


/// The larger container that holds all data structure obtained from the ACPI table, 
/// such as HPET, FADT, etc. 
pub struct Acpi {
    pub fadt: RwLock<Option<Fadt>>,
    pub hpet: RwLock<Option<BoxRefMut<MappedPages, Hpet>>>,
    // pub namespace: RwLock<Option<BTreeMap<String, AmlValue>>>,
    pub next_ctx: RwLock<u64>,
}

static ACPI_TABLE: Acpi = Acpi {
    fadt: RwLock::new(None),
    hpet: RwLock::new(None),
    // namespace: RwLock::new(None),
    next_ctx: RwLock::new(0),
};

lazy_static! {
    static ref ACPI_TABLE_MAPPED_PAGES: Mutex<BTreeMap<Frame, MappedPages>> = Mutex::new(BTreeMap::new());
}



fn get_sdt(sdt_address: PhysicalAddress, active_table: &mut ActivePageTable) -> Result<&'static Sdt, &'static str> {
    
    let mut allocator = try!(FRAME_ALLOCATOR.try().ok_or("Couldn't get Frame Allocator")).lock();
    let addr_offset = address_page_offset(sdt_address);
    let first_frame = Frame::containing_address(sdt_address);

    // first, make sure the given sdt_address is mapped to a virtual memory Page, so we can access it
    let sdt_virt_addr = {
        let opt: Option<VirtualAddress> = {
            let frame_to_page_mappings = ACPI_TABLE_MAPPED_PAGES.lock();
            if let Some(mapped_page) = frame_to_page_mappings.get(&first_frame) {
                // the given sdt_address has already been mapped
                // trace!("get_sdt(): sdt_address {:#X} ({:?}) was already mapped to Page {:?}.", sdt_address, first_frame, mapped_page);
                Some(mapped_page.start_address() + addr_offset)
            }
            else {
                None
            }
        };
        if let Some(vaddr) = opt {
            vaddr
        }
        else {
            // here, the given sdt_address has not yet been mapped, so map it
            let pages = try!(allocate_pages(1).ok_or("couldn't allocate_pages"));
            let mapped_page = try!(active_table.map_allocated_pages_to(
                pages, Frame::range_inclusive(first_frame.clone(), first_frame.clone()), EntryFlags::PRESENT | EntryFlags::NO_EXECUTE, allocator.deref_mut())
            );
            let vaddr = mapped_page.start_address() + addr_offset;
            // trace!("get_sdt(): mapping sdt_address {:#X} ({:?}) to virt_addr {:#X}.", sdt_address, first_frame, vaddr);
            
            ACPI_TABLE_MAPPED_PAGES.lock().insert(first_frame.clone(), mapped_page);

            vaddr
        }
    };

    // SAFE: sdt_virt_addr was mapped above
    let sdt = unsafe { &*(sdt_virt_addr as *const Sdt) };
    // debug!("get_sdt(): sdt = {:?}", sdt);

    // Map extra SDT frames if required
    let end_frame = Frame::containing_address(sdt_address + sdt.length as usize);
    for frame in Frame::range_inclusive(first_frame + 1, end_frame) { // +1 because we already mapped first_frame above
        let mut frame_to_page_mappings = ACPI_TABLE_MAPPED_PAGES.lock();
        {
            if let Some(_mapped_page) = frame_to_page_mappings.get(&frame) {
                // trace!("get_sdt():     extra length sdt_address {:?} was already mapped to {:?}!", frame, _mapped_page);
                continue;
            }
        }

        let pages = try!(allocate_pages(1).ok_or("couldn't allocate_pages"));
        let mapped_page = try!(active_table.map_allocated_pages_to(
            pages, Frame::range_inclusive(frame.clone(), frame.clone()), EntryFlags::PRESENT | EntryFlags::NO_EXECUTE, allocator.deref_mut())
        );
        frame_to_page_mappings.insert(frame, mapped_page);
    }

    Ok(sdt)
}

// fn init_aml_table(sdt: &'static Sdt) {
//     match parse_aml_table(sdt) {
//         Ok(_) => debug!(": Parsed"),
//         Err(AmlError::AmlParseError(e)) => error!(": {}", e),
//         Err(AmlError::AmlInvalidOpCode) => error!(": Invalid opcode"),
//         Err(AmlError::AmlValueError) => error!(": Type constraints or value bounds not met"),
//         Err(AmlError::AmlDeferredLoad) => debug!(": Deferred load reached top level"),
//         Err(AmlError::AmlFatalError(_, _, _)) => {
//             error!(": Fatal error occurred");
//             unsafe { kstop(); }
//         },
//         Err(AmlError::AmlHardFatal) => {
//             error!(": Fatal error occurred");
//             unsafe { kstop(); }
//         }
//     }
// }

// fn init_namespace() {
//     {
//         let mut namespace = ACPI_TABLE.namespace.write();
//         *namespace = Some(BTreeMap::new());
//     }

//     let dsdt = find_sdt("DSDT");
//     if dsdt.len() == 1 {
//         debug!("  DSDT");
//         load_table(get_sdt_signature(dsdt[0]));
//         init_aml_table(dsdt[0]);
//     } else {
//         error!("Unable to find DSDT");
//         return;
//     };

//     let ssdts = find_sdt("SSDT");

//     for ssdt in ssdts {
//         debug!("  SSDT");
//         load_table(get_sdt_signature(ssdt));
//         init_aml_table(ssdt);
//     }
// }

/// Parse the ACPI tables to gather CPU, interrupt, and timer information
pub fn init(active_table: &mut ActivePageTable) -> Result<madt::MadtIter, &'static str> {
    {
        let mut sdt_ptrs = SDT_POINTERS.write();
        *sdt_ptrs = Some(BTreeMap::new());
    }

    {
        let mut order = SDT_ORDER.write();
        *order = Some(vec!());
    }

    // Search for RSDP
    if let Some((rsdp, _rsdp_mapped_pages)) = RSDP::get_rsdp(active_table) {
        // { 
        //     ACPI_TABLE_MAPPED_PAGES.lock().push(_rsdp_mapped_pages);
        // }

        let rxsdt = try!(get_sdt(rsdp.sdt_address(), active_table));
        debug!("rxsdt: {:?}", rxsdt);

        let rxsdt: Box<Rxsdt + Send + Sync> = {
            if let Some(rsdt) = Rsdt::new(rxsdt) {
                Box::new(rsdt)
            } else if let Some(xsdt) = Xsdt::new(rxsdt) {
                Box::new(xsdt)
            } else {
                error!("UNKNOWN RSDT OR XSDT SIGNATURE");
                return Err("unknown rsdt/xsdt signature!");
            }
        };

        // inform the frame allocator that the physical frames where the top-level RSDT/XSDT table exists
        // is now off-limits and should not be touched
        {
            let rxsdt_area = PhysicalMemoryArea::new(rsdp.sdt_address() as usize, rxsdt.length(), 1, 3); // TODO: FIXME:  use proper acpi number 
            try!(
                try!(FRAME_ALLOCATOR.try().ok_or("Couldn't get FRAME ALLOCATOR")).lock().add_area(rxsdt_area, false)
            );
        }

        try!(rxsdt.map_all(active_table));

        {
            let mapped_pages = &*ACPI_TABLE_MAPPED_PAGES.lock();
            debug!("ACPI_TABLE_MAPPED_PAGES = {:?}", mapped_pages);
        }


        for sdt_paddr in rxsdt.iter() {
            let sdt_vaddr: VirtualAddress = {
                if let Some(page) = ACPI_TABLE_MAPPED_PAGES.lock().get(&Frame::containing_address(sdt_paddr)) {
                    page.start_address() + address_page_offset(sdt_paddr)
                }
                else {
                    error!("acpi::init(): ACPI_TABLE_MAPPED_PAGES didn't include a mapping for sdt_paddr: {:#X}", sdt_paddr);
                    return Err("acpi::init(): ACPI_TABLE_MAPPED_PAGES didn't include a mapping for every sdt_paddr");
                }
            };
            let sdt = unsafe { &*(sdt_vaddr as *const Sdt) };

            let signature = get_sdt_signature(sdt);
            if let Some(ref mut ptrs) = *(SDT_POINTERS.write()) {
                ptrs.insert(signature, sdt);
            }
        }

        // FADT is mandatory
        try!(Fadt::init(active_table));
        
        // HPET is optional
        if let Ok(mut hpet) = hpet::init(active_table) {
            let mut hpet_entry = ACPI_TABLE.hpet.write();
            *hpet_entry = Some(hpet);
        }
        else {
            warn!("This machine has no HPET, skipping HPET init.");
        }
        

        // MADT is mandatory
        let madt_iter = Madt::init(active_table);
        // Dmar::init(active_table);
        // init_namespace();

        madt_iter

        // _rsdp_mapped_pages is dropped here and auto-unmapped

    } 
    else {
        error!("NO RSDP FOUND");
        Err("could not find RSDP")
    }
}


/// Returns a reference to the HPET timer structure, wrapped in an Option,
/// because it is not guaranteed that HPET exists or has been initialized.
/// # Example
/// ```
/// let counter_val = get_hpet().as_ref().unwrap().get_counter();
/// ```
pub fn get_hpet() -> RwLockReadGuard<'static, Option<BoxRefMut<MappedPages, Hpet>>> {
    ACPI_TABLE.hpet.read()
}

/// Returns a mutable reference to the HPET timer structure, wrapped in an Option,
/// because it is not guaranteed that HPET exists or has been initialized.
/// # Example
/// ```
/// get_hpet_mut().as_mut().unwrap().enable_counter(true);
/// ```
pub fn get_hpet_mut() -> RwLockWriteGuard<'static, Option<BoxRefMut<MappedPages, Hpet>>> {
    ACPI_TABLE.hpet.write()
}


// pub fn set_global_s_state(state: u8) {
//     if state == 5 {
//         let fadt = ACPI_TABLE.fadt.read();

//         if let Some(ref fadt) = *fadt {
//             let port = fadt.pm1a_control_block as u16;
//             let mut val = 1 << 13;

//             let namespace = ACPI_TABLE.namespace.read();

//             if let Some(ref namespace) = *namespace {
//                 if let Some(s) = namespace.get("\\_S5") {
//                     if let Ok(p) = s.get_as_package() {
//                         let slp_typa = p[0].get_as_integer().expect("SLP_TYPa is not an integer");
//                         let slp_typb = p[1].get_as_integer().expect("SLP_TYPb is not an integer");

//                         info!("Shutdown SLP_TYPa {:X}, SLP_TYPb {:X}", slp_typa, slp_typb);
//                         val |= slp_typa as u16;

//                         info!("Shutdown with ACPI outw(0x{:X}, 0x{:X})", port, val);
//                         Pio::<u16>::new(port).write(val);
//                     }
//                 }
//             }
//         }
//     }
// }

type SdtSignature = (String, [u8; 6], [u8; 8]);
pub static SDT_POINTERS: RwLock<Option<BTreeMap<SdtSignature, &'static Sdt>>> = RwLock::new(None);
pub static SDT_ORDER: RwLock<Option<Vec<SdtSignature>>> = RwLock::new(None);

pub fn find_sdt(name: &str) -> Vec<&'static Sdt> {
    let mut sdts: Vec<&'static Sdt> = vec!();

    if let Some(ref ptrs) = *(SDT_POINTERS.read()) {
        for (signature, sdt) in ptrs {
            if signature.0 == name {
                sdts.push(sdt);
            }
        }
    }

    sdts
}

pub fn get_sdt_signature(sdt: &'static Sdt) -> SdtSignature {
    let signature = String::from_utf8(sdt.signature.to_vec()).expect("Error converting signature to string");
    (signature, sdt.oem_id, sdt.oem_table_id)
}

pub fn load_table(signature: SdtSignature) {
    let mut order = SDT_ORDER.write();

    if let Some(ref mut o) = *order {
        o.push(signature);
    }
}

pub fn get_signature_from_index(index: usize) -> Option<SdtSignature> {
    if let Some(ref order) = *(SDT_ORDER.read()) {
        if index < order.len() {
            Some(order[index].clone())
        } else {
            None
        }
    } else {
        None
    }
}

pub fn get_index_from_signature(signature: SdtSignature) -> Option<usize> {
    if let Some(ref order) = *(SDT_ORDER.read()) {
        let mut i = order.len();
        while i > 0 {
            i -= 1;

            if order[i] == signature {
                return Some(i);
            }
        }
    }

    None
}
