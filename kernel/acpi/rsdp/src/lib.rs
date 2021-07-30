//! Support for ACPI RSDP (Root System Descriptor Pointer).

#![no_std]

extern crate alloc;
extern crate memory;
extern crate owning_ref;
extern crate zerocopy;

use core::mem;
use memory::{PageTable, MappedPages, PhysicalAddress, allocate_pages_by_bytes, allocate_frames_by_bytes_at, EntryFlags};
use owning_ref::BoxRef;
use alloc::boxed::Box;
use zerocopy::FromBytes;

/// The starting physical address of the region of memory where the RSDP table exists.
const RSDP_SEARCH_START: usize = 0xE_0000;
/// The ending physical address of the region of memory where the RSDP table exists.
const RSDP_SEARCH_END:   usize = 0xF_FFFF;
/// The byte-string signature of the RSDP in memory.
const RSDP_SIGNATURE: &'static [u8; 8] = b"RSD PTR ";
/// The RSDP signature is always aligned on a 16-byte boundary.
const RSDP_SIGNATURE_ALIGNMENT: usize = 16;


/// The Root System Descriptor Pointer, 
/// which contains the address of the RSDT (or XSDT),
/// among other items.  
#[derive(Copy, Clone, Debug, FromBytes)]
#[repr(packed)]
pub struct Rsdp {
    signature: [u8; 8],
    checksum: u8,
    oemid: [u8; 6],
    revision: u8,
    rsdt_address: u32,
    length: u32,
    xsdt_address: u64,
    extended_checksum: u8,
    reserved: [u8; 3]
}

impl Rsdp {
    /// Search for the RSDP in the BIOS memory area from 0xE_0000 to 0xF_FFFF.
    /// Returns the RSDP structure and the pages that are currently mapping it.
    pub fn get_rsdp(page_table: &mut PageTable) -> Result<BoxRef<MappedPages, Rsdp>, &'static str> {
        let size: usize = RSDP_SEARCH_END - RSDP_SEARCH_START;
        let pages = allocate_pages_by_bytes(size).ok_or("couldn't allocate pages")?;
        let frames_to_search = allocate_frames_by_bytes_at(PhysicalAddress::new_canonical(RSDP_SEARCH_START), size)
            .map_err(|_e| "Couldn't allocate physical frames when searching for RSDP")?;
        let mapped_pages = page_table.map_allocated_pages_to(pages, frames_to_search, EntryFlags::PRESENT)?;
        Rsdp::search(mapped_pages)
    }

    /// Searches a region of memory for the RSDP, which is identified by the "RSD PTR " signature.
    fn search(region: MappedPages) -> Result<BoxRef<MappedPages, Rsdp>, &'static str> {
        let size = region.size_in_bytes() - mem::size_of::<Rsdp>();
        let signature_length = mem::size_of_val(RSDP_SIGNATURE);
        let mut found_offset: Option<usize> = None;

        {        
            let region_slice: &[u8] = region.as_slice(0, size)?;
            for offset in (0 .. size).step_by(RSDP_SIGNATURE_ALIGNMENT) {
                if &region_slice[offset .. (offset + signature_length)] == &*RSDP_SIGNATURE {
                    found_offset = Some(offset);
                }
            }
        }

        found_offset
            .ok_or("couldn't find RSDP signature in BIOS memory")
            .and_then(|offset| BoxRef::new(Box::new(region)).try_map(|mp| mp.as_type::<Rsdp>(offset)))
    }

    /// Returns the `PhysicalAddress` of the RSDT or XSDT.
    pub fn sdt_address(&self) -> PhysicalAddress {
        if self.revision >= 2 {
            PhysicalAddress::new_canonical(self.xsdt_address as usize)
        } else {
            PhysicalAddress::new_canonical(self.rsdt_address as usize)
        }
    }
}
