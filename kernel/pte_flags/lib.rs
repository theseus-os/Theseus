#![no_std]

use bitflags::bitflags;

bitflags! {
    pub struct MappedPageAttributes: u8 {
        const VALID_ENTRY = 0b0000_0001;
        const WRITEABLE   = 0b0000_0010;
        const EXECUTABLE  = 0b0000_0100;
        const CACHEABLE   = 0b0000_1000;
        const GLOBAL      = 0b0001_0000;
        const EXCLUSIVE   = 0b0010_0000;
        const DEFAULT = Self::VALID_ENTRY.bits | Self::CACHEABLE.bits;
    }
}

impl MappedPageAttributes {
    fn cond(self, enable: bool, flag: MappedPageAttributes) -> Self {
        self & !flag | match enable {
            true => flag,
            false => Self::empty(),
        }
    }

    fn has(self, flag: MappedPageAttributes, shift: u64, yes_bits: u64, no_bits:u64) -> u64 {
        match self.contains(flag) {
            true => yes_bits << shift,
            false => no_bits << shift,
        }
    }

    // The processor will stop a page
    // table walk upon encountering a
    // slot without this bit.
    pub fn valid(self, enable: bool) -> Self {
        self.cond(enable, Self::VALID_ENTRY)
    }

    // The mapped page can be written to
    pub fn writeable(self, enable: bool) -> Self {
        self.cond(enable, Self::WRITEABLE)
    }

    // The mapped page contains code to
    // be jumped to
    pub fn executable(self, enable: bool) -> Self {
        self.cond(enable, Self::EXECUTABLE)
    }

    // The mapped page can be stored in
    // internal CPU caches (like the L2
    // cache) for faster access (this
    // process is automatic)
    pub fn cacheable(self, enable: bool) -> Self {
        self.cond(enable, Self::CACHEABLE)
    }

    // This mapped page is mapped in
    // all address spaces
    pub fn global(self, enable: bool) -> Self {
        self.cond(enable, Self::GLOBAL)
    }

    // This mapped page is owned by this
    // address space and cannot be mapped
    // in other address spaces
    pub fn exclusive(self, enable: bool) -> Self {
        self.cond(enable, Self::EXCLUSIVE)
    }

    pub fn is_valid(&self) -> bool {
        self.contains(Self::VALID_ENTRY)
    }

    pub fn is_writeable(&self) -> bool {
        self.contains(Self::WRITEABLE)
    }

    pub fn is_executable(&self) -> bool {
        self.contains(Self::EXECUTABLE)
    }

    pub fn is_cacheable(&self) -> bool {
        self.contains(Self::CACHEABLE)
    }

    pub fn is_global(&self) -> bool {
        self.contains(Self::GLOBAL)
    }

    pub fn is_exclusive(&self) -> bool {
        self.contains(Self::EXCLUSIVE)
    }
}

#[cfg(target_arch = "aarch64")]
impl MappedPageAttributes {
    pub fn to_hardware(self) -> u64 {
        let mut hw = 0;

        // with the mandatory NOT_A_BLOCK flag
        hw |= self.has(Self::VALID_ENTRY,  0, 0b11, 0b10);
        hw |= self.has(Self::WRITEABLE,    7,  0b0,  0b1);

        // only one bit is used when the cpu supports
        // one privilege level; when two are supported,
        // we disable execution for both
        hw |= self.has(Self::EXECUTABLE,  53, 0b00, 0b11);

        // this assumes the MAIR register has a first
        // entry for non cacheable/device memory and
        // a second entry for cacheable memory;
        // additionally, it sets the entry as describing
        // a page that has inner shareability.
        //
        //              shareable   MAIR index
        let cacheable = 0b11 << 8 | 0b1 << 2;
        let no_cache  = 0b00 << 8 | 0b0 << 2;
        hw |= self.has(Self::CACHEABLE,   0, cacheable, no_cache);

        hw |= self.has(Self::GLOBAL,      11,  0b0,  0b1);
        hw |= self.has(Self::EXCLUSIVE,   55,  0b1,  0b0);

        hw
    }
}

#[cfg(target_arch = "x86_64")]
impl MappedPageAttributes {
    pub fn to_hardware(self) -> u64 {
        let mut hw = 0;

        hw |= self.has(Self::VALID_ENTRY,  0,  0b1,  0b0);
        hw |= self.has(Self::WRITEABLE,    1,  0b1,  0b0);
        hw |= self.has(Self::EXECUTABLE,  63,  0b0,  0b1);
        hw |= self.has(Self::CACHEABLE,    4,  0b0,  0b1);
        hw |= self.has(Self::GLOBAL,       8,  0b1,  0b0);
        hw |= self.has(Self::EXCLUSIVE,    9,  0b1,  0b0);

        hw
    }
}
