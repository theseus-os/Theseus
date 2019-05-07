use memory::{ActivePageTable, MappedPages, Frame, FRAME_ALLOCATOR, PhysicalAddress, allocate_pages_by_bytes, EntryFlags};
use core::ops::DerefMut;
use core::mem;
use owning_ref::BoxRef;
use alloc::boxed::Box;

/// The starting physical address of the region of memory where the RDSP table exists.
const RDSP_SEARCH_START: usize = 0xE_0000;
/// The ending physical address of the region of memory where the RDSP table exists.
const RDSP_SEARCH_END:   usize = 0xF_FFFF;

/// RSDP
#[derive(Copy, Clone, Debug)]
#[repr(packed)]
pub struct RSDP {
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

impl RSDP {
    /// Search for the RSDP in the BIOS memory area from 0xE_0000 to 0xF_FFFF.
    /// Returns the RDSP structure and the pages that are currently mapping it.
    pub fn get_rsdp(active_table: &mut ActivePageTable) -> Option<BoxRef<MappedPages, RSDP>> {
        let size: usize = RDSP_SEARCH_END - RDSP_SEARCH_START;
        let pages = try_opt!(allocate_pages_by_bytes(size));
        let search_range = Frame::range_inclusive(
            Frame::containing_address(PhysicalAddress::new_canonical(RDSP_SEARCH_START)),
            Frame::containing_address(PhysicalAddress::new_canonical(RDSP_SEARCH_END))
        );
        
        let mapped_pages = {
            let allocator_mutex = try_opt!(FRAME_ALLOCATOR.try());
            let mut allocator = allocator_mutex.lock();
            try_opt!(active_table.map_allocated_pages_to(pages, search_range, EntryFlags::PRESENT, allocator.deref_mut()).ok())
        };
        
        RSDP::search(mapped_pages)
    }

    /// Searches a region of memory for thee RSDP table, which is identified by the "RSD PTR " signature.
    fn search(region: MappedPages) -> Option<BoxRef<MappedPages, RSDP>> {
        let size = region.size_in_bytes() - mem::size_of::<RSDP>();
        let mut found_offset: Option<usize> = None;
        for offset in (0 .. size).step_by(16) {
            if let Ok(rsdp) = region.as_type::<RSDP>(offset) {
                if &rsdp.signature == b"RSD PTR " {
                    found_offset = Some(offset);
                }
            }
        }
        found_offset.and_then(|off| BoxRef::new(Box::new(region)).try_map(|mp| mp.as_type::<RSDP>(off)).ok())
    }

    /// Get the RSDT or XSDT address
    pub fn sdt_address(&self) -> PhysicalAddress {
        if self.revision >= 2 {
            PhysicalAddress::new_canonical(self.xsdt_address as usize)
        } else {
            PhysicalAddress::new_canonical(self.rsdt_address as usize)
        }
    }
}
