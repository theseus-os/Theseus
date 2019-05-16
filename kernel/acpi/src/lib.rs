//! Code to parse the ACPI tables, based off of Redox. 
#![no_std]
#![feature(const_fn)]
#![feature(asm)]

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
extern crate pause;
extern crate acpi_table;
extern crate acpi_table_handler;
extern crate sdt;
extern crate rsdp;
extern crate rsdt;




use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use alloc::boxed::Box;
use core::ops::DerefMut;
use spin::{Mutex, RwLock};
use memory::{PageTable, allocate_pages, MappedPages, PhysicalMemoryArea, VirtualAddress, PhysicalAddress, Frame, FrameIter, EntryFlags, FRAME_ALLOCATOR};
use owning_ref::BoxRef;
use rsdp::Rsdp;

pub use self::fadt::Fadt;
pub use self::madt::Madt;
use acpi_table::AcpiTables;
use acpi_table_handler::acpi_table_handler;
use sdt::Sdt;


mod fadt;
pub mod madt;




lazy_static! {
    /// The singleton instance of the `AcpiTables` struct,
    /// which contains the MappedPages and location of all discovered ACPI tables.
    static ref ACPI_TABLES: Mutex<AcpiTables> = Mutex::new(AcpiTables::default());
}



/// The larger container that holds all data structure obtained from the ACPI table.
pub struct Acpi {
    pub fadt: RwLock<Option<Fadt>>,
}

static ACPI_TABLE: Acpi = Acpi {
    fadt: RwLock::new(None),
};


lazy_static! {
    /// The MappedPages that cover all APIC SDT (System Descriptor Tables).
    /// This variable contains a mapping from the physical memory frame holding the SDT's physical address
    /// to the virtual memory page (MappedPages) that cover it. 
    static ref ACPI_TABLE_MAPPED_PAGES: Mutex<BTreeMap<Frame, MappedPages>> = Mutex::new(BTreeMap::new());
}



fn get_and_map_sdt(
    sdt_address: PhysicalAddress,
    page_table: &mut PageTable
) -> Result<&'static Sdt, &'static str> {
// ) -> Result<BoxRef<MappedPages, Sdt>, &'static str> {
    
    debug!("Mapping SDT at paddr: {:#X}", sdt_address);
    let addr_offset = sdt_address.frame_offset();
    let first_frame = Frame::containing_address(sdt_address);

    // first, make sure the given `sdt_address` is mapped to a virtual memory page, so we can access it
    let sdt_virt_addr = {
        let opt: Option<VirtualAddress> = {
            let frame_to_page_mappings = ACPI_TABLE_MAPPED_PAGES.lock();
            if let Some(mapped_page) = frame_to_page_mappings.get(&first_frame) {
                // the given sdt_address has already been mapped
                trace!("get_and_map_sdt(): sdt_address {:#X} ({:?}) was already mapped to Page {:?}.", sdt_address, first_frame, mapped_page);
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
            let mut allocator = FRAME_ALLOCATOR.try().ok_or("Couldn't get Frame Allocator")?.lock();
            let mapped_page = try!(page_table.map_allocated_pages_to(
                pages, Frame::range_inclusive(first_frame.clone(), first_frame.clone()), EntryFlags::PRESENT | EntryFlags::NO_EXECUTE, allocator.deref_mut())
            );
            let vaddr = mapped_page.start_address() + addr_offset;
            trace!("get_and_map_sdt(): mapping sdt_address {:#X} ({:?}) to virt_addr {:#X}.", sdt_address, first_frame, vaddr);
            
            ACPI_TABLE_MAPPED_PAGES.lock().insert(first_frame.clone(), mapped_page);

            vaddr
        }
    };

    // SAFE: sdt_virt_addr was mapped above
    let sdt = unsafe { &*(sdt_virt_addr.value() as *const Sdt) };
    debug!("SDT's length is {}, signature: {}", sdt.length, core::str::from_utf8(&sdt.signature).unwrap_or("Unknown"));

    // Map extra SDT frames if required
    let end_frame = Frame::containing_address(sdt_address + sdt.length as usize);
    for frame in Frame::range_inclusive(first_frame + 1, end_frame) { // +1 because we already mapped first_frame above
        warn!("get_and_map_sdt():     SDT's extra length requires mapping frame {:?}!", frame);
        let mut frame_to_page_mappings = ACPI_TABLE_MAPPED_PAGES.lock();
        {
            if let Some(_mapped_page) = frame_to_page_mappings.get(&frame) {
                trace!("get_and_map_sdt():     extra length sdt_address {:?} was already mapped to {:?}!", frame, _mapped_page);
                continue;
            }
        }

        let pages = try!(allocate_pages(1).ok_or("couldn't allocate_pages"));
        let mut allocator = FRAME_ALLOCATOR.try().ok_or("Couldn't get Frame Allocator")?.lock();
        let mapped_page = try!(page_table.map_allocated_pages_to(
            pages, Frame::range_inclusive(frame.clone(), frame.clone()), EntryFlags::PRESENT | EntryFlags::NO_EXECUTE, allocator.deref_mut())
        );
        frame_to_page_mappings.insert(frame, mapped_page);
    }

    Ok(sdt)
}


/// Parses the system's ACPI tables 
pub fn init(page_table: &mut PageTable) -> Result<madt::MadtIter, &'static str> {
    {
        let mut sdt_ptrs = SDT_POINTERS.write();
        *sdt_ptrs = Some(BTreeMap::new());
    }

    // The first step is to search for the RSDP (Root System Descriptor Pointer),
    // which contains the physical address of the RSDT/XSDG (Root/Extended System Descriptor Table).
    let rsdp = Rsdp::get_rsdp(page_table)?;
    let rsdt_phys_addr = rsdp.sdt_address();
    debug!("RXSDT is located in Frame {:#X}", rsdt_phys_addr);

    // Now, we get the actual RSDT/XSDT
    let mut acpi_tables = ACPI_TABLES.lock();
    let (sdt_signature, sdt_total_length) = acpi_tables.map_new_table(rsdt_phys_addr, page_table)?;
    acpi_table_handler(&mut acpi_tables, sdt_signature, sdt_total_length, rsdt_phys_addr)?;
    let rxsdt = rsdt::RsdtXsdt::get(&acpi_tables).ok_or("couldn't get RSDT or XSDT from ACPI tables")?;

    // The RSDT/XSDT tells us where all of the rest of the ACPI tables exist.
    for (i, sdt_paddr) in rxsdt.addresses().enumerate() {
        debug!("RXSDT[{}]: {:#X}", i, sdt_paddr);
        get_and_map_sdt(sdt_paddr, page_table)?;
    }

    for sdt_paddr in rxsdt.addresses() {
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
    Fadt::init(page_table)?;
    
    // HPET is optional
    let hpet_result = {
        let hpet_sdt = find_matching_sdts("HPET");
        if hpet_sdt.len() == 1 {
            hpet::init(hpet_sdt[0], page_table)
        }
        else {
            Err("unable to find HPET SDT")
        }
    };
    if let Err(_e) = hpet_result {
        warn!("This machine has no HPET.");
    }
    

    // MADT is mandatory
    let madt_iter = Madt::init(page_table);
    // Dmar::init(page_table);
    // init_namespace();

    madt_iter

    // _rsdp_mapped_pages is dropped here and auto-unmapped
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
