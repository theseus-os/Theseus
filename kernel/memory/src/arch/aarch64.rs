extern crate aarch64;

pub use self::aarch64::instructions::tlb;
pub use kernel_config::memory::aarch64::{KERNEL_OFFSET, KERNEL_OFFSET_BITS_START, KERNEL_OFFSET_PREFIX, HARDWARE_START, HARDWARE_END};

pub fn rw_entry_flags() -> EntryFlags {
    EntryFlags::PRESENT | EntryFlags::WRITABLE | EntryFlags::ACCESSEDARM | EntryFlags::INNER_SHARE
}

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