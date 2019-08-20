extern crate aarch64;

pub use self::aarch64::instructions::tlb;
pub use kernel_config::memory::aarch64::{KERNEL_OFFSET, KERNEL_OFFSET_BITS_START, KERNEL_OFFSET_PREFIX, HARDWARE_START, HARDWARE_END};

use super::super::{Frame, PhysicalAddress};


bitflags! {
    #[derive(Default)]
    pub struct EntryFlags: u64 {
        const PRESENT           = 1 << 0;
        const WRITABLE          = 1 << 1;
        const USER_ACCESSIBLE   = 1 << 2;
        // const WRITE_THROUGH     = 1 << 3;
        const NO_CACHE          = 1 << 4;
        // const ACCESSED          = 1 << 5;
        // const DIRTY             = 1 << 6;
        const HUGE_PAGE         = 1 << 7;
        //const GLOBAL            = 1 << 8;
        const GLOBAL            = 0; // disabling because VirtualBox doesn't like it
        const NO_EXECUTE        = 1 << 63;

        //ARM MMU
        const PAGE              = 1 << 1;
        const DEVICE            = 1 << 2;
        const NON_CACHE         = 1 << 3;
        const USER_ARM          = 1 << 6;
        const READONLY          = 1 << 7;
        const OUT_SHARE         = 2 << 8;
        const INNER_SHARE       = 3 << 8;
        const ACCESSEDARM       = 1 << 10;
        const NO_EXE_ARM        = 1 << 54;
    }
    
}

impl EntryFlags {
    pub fn is_huge(&self) -> bool {
        !self.contains(EntryFlags::PAGE)
    }

    pub fn rw_flags() -> EntryFlags {
        EntryFlags::default()
    }

    pub fn default() -> EntryFlags {
        EntryFlags::PRESENT | EntryFlags :: ACCESSEDARM | EntryFlags::INNER_SHARE | EntryFlags::PAGE
    }

    pub fn is_page(&self) -> bool {
        self.contains(EntryFlags::PRESENT) && self.contains(EntryFlags::PAGE)
    }
}

/// Set the p4 address of the new page table
pub fn set_new_p4(p4: u64) {
    unsafe {
        asm!("
        msr ttbr1_el1, x0;
        msr ttbr0_el1, x0;
        dsb ish; 
        isb; " : :"{x0}"(p4): : "volatile");
        tlb::flush_all();
    }
}


/// Returns the current top-level page table frame e.g., TTBR0_EL1 on ARM64
pub fn get_current_p4() -> Frame {
    let p4:usize;
    unsafe {  asm!("mrs $0, TTBR0_EL1" : "=r"(p4) : : : "volatile"); };
    Frame::containing_address(PhysicalAddress::new_canonical(p4))
}


pub fn flush(address:usize) {
    tlb::flush(aarch64::VirtualAddress(address));
}