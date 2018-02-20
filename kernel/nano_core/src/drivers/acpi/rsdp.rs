use memory::{ActivePageTable, MappedPages, Frame, FRAME_ALLOCATOR, PhysicalAddress, allocate_pages_by_bytes, EntryFlags};
use core::ops::DerefMut;

const RDSP_SEARCH_START: PhysicalAddress = 0xE_0000;
const RDSP_SEARCH_END:   PhysicalAddress = 0xF_FFFF;

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
    pub fn get_rsdp(active_table: &mut ActivePageTable) -> Option<(RSDP, MappedPages)> {
        let size: usize = RDSP_SEARCH_END - RDSP_SEARCH_START;
        let pages = try_opt!(allocate_pages_by_bytes(size));
        let search_range = Frame::range_inclusive(
            Frame::containing_address(RDSP_SEARCH_START),
            Frame::containing_address(RDSP_SEARCH_END)
        );
        
        let mapped_pages = {
            let allocator_mutex = try_opt!(FRAME_ALLOCATOR.try());
            let mut allocator = allocator_mutex.lock();
            try_opt!(active_table.map_allocated_pages_to(pages, search_range, EntryFlags::PRESENT, allocator.deref_mut()).ok())
        };
        let rsdp = RSDP::search(mapped_pages.start_address(), mapped_pages.start_address() + size);
        debug!("Found RSDP at addr {:#X}: {:?}", &rsdp as *const _ as usize, rsdp);
        
        rsdp.and_then(|r| Some((r, mapped_pages)))
    }

    fn search(start_addr: usize, end_addr: usize) -> Option<RSDP> {
        for i in 0 .. (end_addr + 1 - start_addr)/16 {
            let rsdp = unsafe { &*((start_addr + i * 16) as *const RSDP) };
            if &rsdp.signature == b"RSD PTR " {
                return Some(*rsdp);
            }
        }
        None
    }

    /// Get the RSDT or XSDT address
    pub fn sdt_address(&self) -> usize {
        if self.revision >= 2 {
            self.xsdt_address as usize
        } else {
            self.rsdt_address as usize
        }
    }
}
