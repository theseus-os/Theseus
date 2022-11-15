#![no_std]

#[repr(transparent)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct PagingEntryFlags(u64);

impl PagingEntryFlags {
    pub fn new(
        valid_entry: bool,
        writeable: bool,
        executable: bool,
        cacheable: bool,
        global: bool,
        userland: bool,
        exclusive: bool,
    ) -> Self {
        let mut flags = 0;

        flags |= match valid_entry {
            true  => arch::USED_ENTRY,
            false => arch::UNUSED_ENTRY,
        };

        flags |= match writeable {
            true  => arch::WRITEABLE,
            false => arch::NON_WRITEABLE,
        };

        flags |= match executable {
            true  => arch::EXECUTABLE,
            false => arch::NON_EXECUTABLE,
        };

        flags |= match cacheable {
            true  => arch::CACHEABLE,
            false => arch::NON_CACHEABLE,
        };

        flags |= match global {
            true  => arch::GLOBAL,
            false => arch::NON_GLOBAL,
        };

        flags |= match userland {
            true  => arch::USERLAND_ACCESSIBLE,
            false => arch::USERLAND_INACCESSIBLE,
        };

        flags |= match exclusive {
            true  => EXCLUSIVE,
            false => NON_EXCLUSIVE,
        };

        Self(flags)
    }

    fn clear_and_set(self, pair: (u64, u64)) -> Self {
        let (to_clear, to_set) = pair;
        Self((self.0 & !to_clear) | to_set)
    }

    pub fn valid(self, valid: bool) -> Self {
        self.clear_and_set(match valid {
            true  => (arch::UNUSED_ENTRY, arch::USED_ENTRY),
            false => (arch::USED_ENTRY, arch::UNUSED_ENTRY),
        })
    }

    pub fn writeable(self, writeable: bool) -> Self {
        self.clear_and_set(match writeable {
            true  => (arch::NON_WRITEABLE, arch::WRITEABLE),
            false => (arch::WRITEABLE, arch::NON_WRITEABLE),
        })
    }

    pub fn executable(self, executable: bool) -> Self {
        self.clear_and_set(match executable {
            true  => (arch::NON_EXECUTABLE, arch::EXECUTABLE),
            false => (arch::EXECUTABLE, arch::NON_EXECUTABLE),
        })
    }

    pub fn cacheable(self, cacheable: bool) -> Self {
        self.clear_and_set(match cacheable {
            true  => (arch::NON_CACHEABLE, arch::CACHEABLE),
            false => (arch::CACHEABLE, arch::NON_CACHEABLE),
        })
    }

    pub fn global(self, global: bool) -> Self {
        self.clear_and_set(match global {
            true  => (arch::NON_GLOBAL, arch::GLOBAL),
            false => (arch::GLOBAL, arch::NON_GLOBAL),
        })
    }

    pub fn userland(self, userland: bool) -> Self {
        self.clear_and_set(match userland {
            true  => (arch::USERLAND_INACCESSIBLE, arch::USERLAND_ACCESSIBLE),
            false => (arch::USERLAND_ACCESSIBLE, arch::USERLAND_INACCESSIBLE),
        })
    }

    pub fn exclusive(self, exclusive: bool) -> Self {
        self.clear_and_set(match exclusive {
            true  => (NON_EXCLUSIVE, EXCLUSIVE),
            false => (EXCLUSIVE, NON_EXCLUSIVE),
        })
    }
}

#[cfg(target_arch = "aarch64")]
mod arch {
    // with the mandatory NOT_A_BLOCK flag
    pub const            USED_ENTRY: u64 = 0b11 <<  0;
    pub const          UNUSED_ENTRY: u64 = 0b10 <<  0;

    pub const             WRITEABLE: u64 =  0b0 <<  7;
    pub const         NON_WRITEABLE: u64 =  0b1 <<  7;

    // only one bit is used when the cpu supports
    // one privilege level; when two are supported,
    // we disable execution for both
    pub const            EXECUTABLE: u64 = 0b00 << 53;
    pub const        NON_EXECUTABLE: u64 = 0b11 << 53;

    // this assumes the MAIR register has a first
    // entry for non cacheable/device memory and
    // a second entry for cacheable memory;
    // additionally, it sets the entry as describing
    // a page that has inner shareability.
    //
    //                                 shareable   MAIR index
    pub const             CACHEABLE: u64 = 0b11 << 8 | 0b1 << 2;
    pub const         NON_CACHEABLE: u64 = 0b00 << 8 | 0b0 << 2;

    pub const                GLOBAL: u64 =  0b0 << 11;
    pub const            NON_GLOBAL: u64 =  0b1 << 11;

    pub const   USERLAND_ACCESSIBLE: u64 =  0b1 << 6;
    pub const USERLAND_INACCESSIBLE: u64 =  0b0 << 6;

    pub const        SOFTWARE_1_SET: u64 =  0b1 << 55;
    pub const      SOFTWARE_1_CLEAR: u64 =  0b0 << 55;
}

#[cfg(target_arch = "x86_64")]
mod arch {
    pub const            USED_ENTRY: u64 =  0b1 <<  0;
    pub const          UNUSED_ENTRY: u64 =  0b0 <<  0;

    pub const             WRITEABLE: u64 =  0b1 <<  1;
    pub const         NON_WRITEABLE: u64 =  0b0 <<  1;

    pub const            EXECUTABLE: u64 =  0b0 << 63;
    pub const        NON_EXECUTABLE: u64 =  0b1 << 63;

    pub const             CACHEABLE: u64 =  0b0 << 4;
    pub const         NON_CACHEABLE: u64 =  0b1 << 4;

    pub const                GLOBAL: u64 =  0b1 << 8;
    pub const            NON_GLOBAL: u64 =  0b0 << 8;

    pub const   USERLAND_ACCESSIBLE: u64 =  0b1 << 2;
    pub const USERLAND_INACCESSIBLE: u64 =  0b0 << 2;

    pub const        SOFTWARE_1_SET: u64 =  0b1 << 9;
    pub const      SOFTWARE_1_CLEAR: u64 =  0b0 << 9;
}

pub const     EXCLUSIVE: u64 = arch::SOFTWARE_1_SET;
pub const NON_EXCLUSIVE: u64 = arch::SOFTWARE_1_CLEAR;
