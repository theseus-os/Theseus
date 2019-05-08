//! Code to parse the ACPI tables, based off of Redox. 
#![no_std]
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
extern crate hpet;
extern crate sdt;


macro_rules! try_opt {
    ($e:expr) =>(
        match $e {
            Some(v) => v,
            None => return None,
        }
    )
}



use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use alloc::boxed::Box;
use core::ops::DerefMut;
use spin::{Mutex, RwLock};


use memory::{ActivePageTable, allocate_pages, MappedPages, PhysicalMemoryArea, VirtualAddress, PhysicalAddress, Frame, EntryFlags, FRAME_ALLOCATOR};

pub use self::fadt::Fadt;
pub use self::madt::Madt;
pub use self::rsdt::Rsdt;
pub use self::xsdt::Xsdt;
pub use self::rxsdt::Rxsdt;
pub use self::rsdp::RSDP;
use sdt::Sdt;


mod fadt;
pub mod madt;
mod rsdt;
mod xsdt;
mod rxsdt;
mod rsdp;


/// The larger container that holds all data structure obtained from the ACPI table.
pub struct Acpi {
    pub fadt: RwLock<Option<Fadt>>,
}

static ACPI_TABLE: Acpi = Acpi {
    fadt: RwLock::new(None),
};

lazy_static! {
    static ref ACPI_TABLE_MAPPED_PAGES: Mutex<BTreeMap<Frame, MappedPages>> = Mutex::new(BTreeMap::new());
}



fn get_sdt(sdt_address: PhysicalAddress, active_table: &mut ActivePageTable) -> Result<&'static Sdt, &'static str> {
    
    let mut allocator = try!(FRAME_ALLOCATOR.try().ok_or("Couldn't get Frame Allocator")).lock();
    let addr_offset = sdt_address.frame_offset();
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
    let sdt = unsafe { &*(sdt_virt_addr.value() as *const Sdt) };
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
    if let Some(rsdp) = RSDP::get_rsdp(active_table) {

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
            let rxsdt_area = PhysicalMemoryArea::new(rsdp.sdt_address(), rxsdt.length(), 1, 3); // TODO: FIXME:  use proper acpi number 
            try!(
                try!(FRAME_ALLOCATOR.try().ok_or("Couldn't get FRAME ALLOCATOR")).lock().add_area(rxsdt_area, false)
            );
        }

        try!(rxsdt.map_all(active_table));

        // {
        //     let _mapped_pages = &*ACPI_TABLE_MAPPED_PAGES.lock();
        //     debug!("ACPI_TABLE_MAPPED_PAGES = {:?}", _mapped_pages);
        // }


        for sdt_paddr in rxsdt.iter() {
            let sdt_paddr = PhysicalAddress::new_canonical(sdt_paddr);
            let sdt_vaddr: VirtualAddress = {
                if let Some(page) = ACPI_TABLE_MAPPED_PAGES.lock().get(&Frame::containing_address(sdt_paddr)) {
                    page.start_address() + sdt_paddr.frame_offset()
                }
                else {
                    error!("acpi::init(): ACPI_TABLE_MAPPED_PAGES didn't include a mapping for sdt_paddr: {:#X}", sdt_paddr);
                    return Err("acpi::init(): ACPI_TABLE_MAPPED_PAGES didn't include a mapping for sdt_paddr");
                }
            };
            let sdt = unsafe { &*(sdt_vaddr.value() as *const Sdt) };

            let signature = get_sdt_signature(sdt);
            if let Some(ref mut ptrs) = *(SDT_POINTERS.write()) {
                ptrs.insert(signature, sdt);
            }
        }

        // FADT is mandatory
        Fadt::init(active_table)?;
        
        // HPET is optional
        let hpet_result = {
            let hpet_sdt = find_matching_sdts("HPET");
            if hpet_sdt.len() == 1 {
                load_table(get_sdt_signature(hpet_sdt[0]));
                hpet::init(hpet_sdt[0], active_table)
            }
            else {
                Err("unable to find HPET SDT")
            }
        };
        if let Err(_e) = hpet_result {
            warn!("This machine has no HPET.");
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

pub fn find_matching_sdts(name: &str) -> Vec<&'static Sdt> {
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
