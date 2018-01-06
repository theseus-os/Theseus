use kernel_config::memory::KERNEL_OFFSET;
use memory::{Frame, ActivePageTable, Page, PhysicalAddress, VirtualAddress, EntryFlags};

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
    /// Search for the RSDP
    pub fn get_rsdp(active_table: &mut ActivePageTable) -> Option<RSDP> {
        let start_addr = 0xE_0000 + KERNEL_OFFSET;
        let end_addr = 0xF_FFFF + KERNEL_OFFSET;

        // The whole area from 0x0 to 0x10_0000 has already been mapped to the higher half in remap_the_kernel()
        // Map all of the ACPI RSDP space
        // {
        //     let start_frame = Frame::containing_address(PhysicalAddress::new(start_addr));
        //     let end_frame = Frame::containing_address(PhysicalAddress::new(end_addr));
        //     for frame in Frame::range_inclusive(start_frame, end_frame) {
        //         let page = Page::containing_address(VirtualAddress::new(frame.start_address().get()));
        //         let result = active_table.map_to(page, frame, EntryFlags::PRESENT | EntryFlags::NO_EXECUTE);
        //         result.flush(active_table);
        //     }
        // }


        RSDP::search(start_addr, end_addr)
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
