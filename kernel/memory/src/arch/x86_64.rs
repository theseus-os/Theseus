extern crate x86_64;

pub use self::x86_64::instructions::tlb;
pub use self::x86_64::registers::control_regs;
pub use self::x86_64::VirtualAddress;

pub use kernel_config::memory::x86_64::{KERNEL_OFFSET, KERNEL_OFFSET_BITS_START, KERNEL_OFFSET_PREFIX};

use super::super::{Frame, PhysicalAddress};

bitflags! {
    #[derive(Default)]
    pub struct EntryFlags: u64 {
        const PRESENT           = 1 << 0;
        const WRITABLE          = 1 << 1;
        const USER_ACCESSIBLE   = 1 << 2;
        const WRITE_THROUGH     = 1 << 3;
        const NO_CACHE          = 1 << 4;
        const ACCESSED          = 1 << 5;
        const DIRTY             = 1 << 6;
        const HUGE_PAGE         = 1 << 7;
        // const GLOBAL            = 1 << 8;
        const GLOBAL            = 0; // disabling because VirtualBox doesn't like it
        const NO_EXECUTE        = 1 << 63;
    }
    
}

impl EntryFlags {
    pub fn is_huge(&self) -> bool {
        self.contains(EntryFlags::HUGE_PAGE)
    }

    pub fn default() -> EntryFlags {
        EntryFlags::PRESENT
    }

    pub fn rw_flags() -> EntryFlags {
        EntryFlags::default() | EntryFlags::WRITABLE
    }
}


/// Set the new P4 table address
pub fn set_new_p4(p4: u64) {
    unsafe {
        control_regs::cr3_write(x86_64::PhysicalAddress(p4));
    }
}


/// Returns the current top-level page table frame, e.g., cr3 on x86
pub fn get_current_p4() -> Frame {
    Frame::containing_address(PhysicalAddress::new_canonical(control_regs::cr3().0 as usize))
}


pub fn flush(address:usize) {
    tlb::flush(x86_64::VirtualAddress(address));
}