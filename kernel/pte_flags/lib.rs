//! This crate defines the structure of page table entry flags on x86_64 & aarch64.
//! 
//! This crate assumes MAIR slot 0 has a
//! "DEVICE nGnRE" entry and slot 1 has a
//! Normal + Outer Shareable entry.

#![no_std]

use bitflags::bitflags;

bitflags! {
    /// Cross-platform, Software flags
    /// for the page table entry flags
    /// we use.
    pub struct PteFlags: u8 {
        /// This page has a frame mapped to it.
        const VALID_ENTRY = 0b0000_0001;

        /// Writes to this location are allowed.
        const WRITABLE    = 0b0000_0010;

        /// Jumping to this location is allowed.
        const EXECUTABLE  = 0b0000_0100;

        /// This mapping can be stored temporarily
        /// in internal CPU caches (L1/L2/L3/etc)
        const CACHEABLE   = 0b0000_1000;

        /// Indicates that this page is
        /// mapped across all address spaces 
        /// (all root page tables) and doesn't
        /// need to be flushed out of the TLB 
        /// when switching to another page table.
        const GLOBAL      = 0b0001_0000;

        /// Indicate that the frame pointed to by
        /// this page table entry is owned **exclusively**
        /// by that page table entry. Currently, in Theseus,
        /// we only set the `EXCLUSIVE` bit for P1-level PTEs
        /// that we **know** are bijective (1-to-1
        /// virtual-to-physical) mappings. If this bit is set,
        /// the pointed frame will be safely deallocated
        /// once this page table entry is unmapped. 
        const EXCLUSIVE   = 0b0010_0000;

        /// The most commonly/normally used flags:
        /// VALID_ENTRY and CACHEABLE
        const DEFAULT = Self::VALID_ENTRY.bits | Self::CACHEABLE.bits;
    }
}

impl PteFlags {
    fn cond(self, enable: bool, flag: Self) -> Self {
        self & !flag | match enable {
            true => flag,
            false => Self::empty(),
        }
    }

    /// The processor will stop a page
    /// table walk upon encountering a
    /// slot without this bit.
    pub fn valid(self, enable: bool) -> Self {
        self.cond(enable, Self::VALID_ENTRY)
    }

    /// The mapped page can be written to
    pub fn writeable(self, enable: bool) -> Self {
        self.cond(enable, Self::WRITABLE)
    }

    /// The mapped page contains code to
    /// be jumped to
    pub fn executable(self, enable: bool) -> Self {
        self.cond(enable, Self::EXECUTABLE)
    }

    /// The mapped page can be stored in
    /// internal CPU caches (like the L2
    /// cache) for faster access (this
    /// process is automatic)
    pub fn cacheable(self, enable: bool) -> Self {
        self.cond(enable, Self::CACHEABLE)
    }

    /// This mapped page is mapped in
    /// all address spaces
    pub fn global(self, enable: bool) -> Self {
        self.cond(enable, Self::GLOBAL)
    }

    /// This mapped page is owned by this
    /// address space and cannot be mapped
    /// in other address spaces
    pub fn exclusive(self, enable: bool) -> Self {
        self.cond(enable, Self::EXCLUSIVE)
    }

    pub fn is_valid(&self) -> bool {
        self.contains(Self::VALID_ENTRY)
    }

    pub fn is_writeable(&self) -> bool {
        self.contains(Self::WRITABLE)
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

pub use arch::ArchSpecificPteFlags;

#[cfg(target_arch = "aarch64")]
mod arch {
    use crate::PteFlags;
    use bitflags::bitflags;

    bitflags! {
        /// Page table entry flags on the aarch64 architecture.
        ///
        /// Based on ARM DDI 0487l.a, page D8-5128
        ///
        /// The designation of bits in each `PageTableEntry` is as such:
        /// * Bits `[0:12]` (inclusive) are reserved by hardware for access flags, cacheability flags, shareability flags and TLB storage flags.
        /// * Bits `[12:51]` (inclusive) are reserved by hardware to hold the physical frame address.
        /// * Bits `[51:55]` (inclusive) are reserved by hardware for TLB storage flags.
        /// * Bits `[55:58]` (inclusive) are available for custom OS usage.
        /// * Bits `[58:63]` (inclusive) are reserved by hardware for extended access flags.
        ///
        /// a _Z suffix indicates that the flag is represented
        /// by cleared bits, so that flag shouldn't be used.
        pub struct ArchSpecificPteFlags: u64 {
            /// This entry contains a valid
            /// page-to-memory mapping.
            const VALID              = 1 << 0;

            /// This either points to a page or
            /// contains an L3 descriptor
            const PAGE_L3_DESCRIPTOR = 1 << 1;
            /// This points to a block table.
            const BLOCK_DESCRIPTOR_Z = 0 << 1;

            /// That mapping's cacheability is
            /// described by MAIR slot 0
            const MAIR_SLOT_0_Z      = 0 << 2;
            /// That mapping's cacheability is
            /// described by MAIR slot 1
            const MAIR_SLOT_1        = 1 << 2;
            /// That mapping's cacheability is
            /// described by MAIR slot 2
            const MAIR_SLOT_2        = 2 << 2;
            /// That mapping's cacheability is
            /// described by MAIR slot 3
            const MAIR_SLOT_3        = 3 << 2;
            /// That mapping's cacheability is
            /// described by MAIR slot 4
            const MAIR_SLOT_4        = 4 << 2;
            /// That mapping's cacheability is
            /// described by MAIR slot 5
            const MAIR_SLOT_5        = 5 << 2;
            /// That mapping's cacheability is
            /// described by MAIR slot 6
            const MAIR_SLOT_6        = 6 << 2;
            /// That mapping's cacheability is
            /// described by MAIR slot 7
            const MAIR_SLOT_7        = 7 << 2;

            const RESERVED_0_Z       = 0 << 5;
            const RESERVED_1_Z       = 0 << 6;

            /// Writes to this page are allowed
            const WRITABLE_Z         = 0 << 7;
            /// Writes to this page are forbidden
            const READ_ONLY          = 1 << 7;

            /// Only one core will ever access
            /// this memory.
            const NON_SHAREABLE_Z    = 0 << 8;
            const RESERVED_SH_VAL    = 1 << 8;
            /// Multiple core clusters can
            /// access this page.
            const OUTER_SHAREABLE    = 2 << 8;
            /// Multiple cores from the same
            /// cluster can access this page.
            const INNER_SHAREABLE    = 3 << 8;

            /// This descriptor should be cached
            /// in a TLB. Updated by the hardware
            /// if the FEAT_HAFDBS optional
            /// feature is implemented.
            const ACCESSED           = 1 << 10;
            /// This descriptor shouldn't be
            /// cached in a TLB. Updated by the
            /// hardware if the FEAT_HAFDBS
            /// optional feature is implemented.
            const NOT_ACCESSED_Z     = 0 << 10;

            /// Indicates that this page is
            /// mapped across all address spaces 
            /// (all root page tables) and doesn't
            /// need to be flushed out of the TLB 
            /// when switching to another page table.
            const GLOBAL_Z           = 0 << 11;
            /// Indicates that this page is not
            /// mapped across all address spaces 
            /// and needs to be flushed out of the
            /// TLB when switching to another page
            /// table.
            const NON_GLOBAL         = 1 << 11;

            /// See D8.4.6 in [DDI0487l.A](https://l0.pm/arm-ddi0487l.a.pdf).
            const CLEAN_Z            = 0 << 51;
            /// See D8.4.6 in [DDI0487l.A](https://l0.pm/arm-ddi0487l.a.pdf).
            const DIRTY              = 1 << 51;

            /// This descriptor and the next in table
            /// describe contiguously mapped memory
            const CONTIGUOUS         = 1 << 52;
            /// This descriptor and the next in table
            /// do not describe contiguously mapped
            /// memory
            const NON_CONTIGUOUS_Z   = 0 << 52;

            /// Privileged execution levels cannot
            /// jump to code in this page.
            const PRIV_EXEC_NEVER    = 1 << 53;
            /// Privileged execution levels can
            /// jump to code in this page.
            const PRIV_CAN_EXEC_Z    = 0 << 53;

            /// Unprivileged execution levels cannot
            /// jump to code in this page.
            const USER_EXEC_NEVER    = 1 << 54;
            /// Unprivileged execution levels can
            /// jump to code in this page.
            const USER_CAN_EXEC_Z    = 0 << 54;

            /// Available for software use
            const SOFTWARE_BIT_1     = 1 << 55;
            /// Available for software use
            const SOFTWARE_BIT_2     = 1 << 56;
            /// Available for software use
            const SOFTWARE_BIT_3     = 1 << 57;
            /// Available for software use
            const SOFTWARE_BIT_4     = 1 << 58;
        }
    }

    type Arch = ArchSpecificPteFlags;

    impl From<PteFlags> for Arch {
        fn from(sw: PteFlags) -> Self {
            let mut hw = Self::empty();

            hw |= match sw.contains(PteFlags::VALID_ENTRY) {
                true => Self::VALID | Self::PAGE_L3_DESCRIPTOR | Self::ACCESSED,
                false => Self::empty(),
            };

            hw |= match sw.contains(PteFlags::WRITABLE) {
                false => Self::READ_ONLY,
                true => Self::empty(),
            };

            hw |= match sw.contains(PteFlags::EXECUTABLE) {
                false => Self::PRIV_EXEC_NEVER | Self::USER_EXEC_NEVER,
                true => Self::empty(),
            };

            // This crate assumes MAIR slot 0 has a
            // "DEVICE nGnRE" entry and slot 1 has a
            /// Normal + Outer Shareable entry.
            hw |= match sw.contains(PteFlags::CACHEABLE) {
                true => Self::OUTER_SHAREABLE | Self::MAIR_SLOT_1,
                false => Self::MAIR_SLOT_0_Z, // => 0
            };

            hw |= match sw.contains(PteFlags::GLOBAL) {
                false => Self::NON_GLOBAL,
                true => Self::empty(),
            };

            hw |= match sw.contains(PteFlags::EXCLUSIVE) {
                true => Self::SOFTWARE_BIT_1,
                false => Self::empty(),
            };

            hw
        }
    }

    impl From<Arch> for PteFlags {
        fn from(hw: Arch) -> Self {
            let mut sw = Self::empty();

            sw |= match hw.contains(Arch::VALID) {
                true => Self::VALID_ENTRY,
                false => Self::empty(),
            };

            sw |= match hw.contains(Arch::READ_ONLY) {
                false => Self::WRITABLE,
                true => Self::empty(),
            };

            sw |= match hw.contains(Arch::PRIV_EXEC_NEVER) {
                false => Self::EXECUTABLE,
                true => Self::empty(),
            };

            sw |= match hw.contains(Arch::OUTER_SHAREABLE) {
                true => Self::CACHEABLE,
                false => Self::empty(),
            };

            sw |= match hw.contains(Arch::NON_GLOBAL) {
                false => Self::GLOBAL,
                true => Self::empty(),
            };

            sw |= match hw.contains(Arch::SOFTWARE_BIT_1) {
                true => Self::EXCLUSIVE,
                false => Self::empty(),
            };

            sw
        }
    }
}

#[cfg(target_arch = "x86_64")]
mod arch {
    use crate::PteFlags;
    use bitflags::bitflags;

    bitflags! {
        /// Page table entry flags on the x86_64 architecture.
        ///
        /// The designation of bits in each `PageTableEntry` is as such:
        /// * Bits `[0:8]` (inclusive) are reserved by hardware for access flags.
        /// * Bits `[9:11]` (inclusive) are available for custom OS usage.
        /// * Bits `[12:51]` (inclusive) are reserved by hardware to hold the physical frame address.
        /// * Bits `[52:62]` (inclusive) are available for custom OS usage.
        /// * Bit  `63` is reserved by hardware for access flags (noexec).
        ///
        pub struct ArchSpecificPteFlags: u64 {
            /// If set, this page is currently "present" in memory. 
            /// If not set, this page is not in memory, e.g., not mapped, paged to disk, etc.
            const PRESENT           = 1 <<  0;
            /// If set, writes to this page are allowed.
            /// If not set, this page is read-only.
            const WRITABLE          = 1 <<  1;
            /// If set, userspace (ring 3) can access this page.
            /// If not set, only kernelspace (ring 0) can access this page. 
            const USER_ACCESSIBLE   = 1 <<  2;
            /// If set, writes to this page go directly through the cache to memory. 
            const WRITE_THROUGH     = 1 <<  3;
            /// If set, this page's content is never cached, neither for read nor writes. 
            const NO_CACHE          = 1 <<  4;
            /// The hardware will set this bit when the page is accessed.
            const ACCESSED          = 1 <<  5;
            /// The hardware will set this bit when the page has been written to.
            const DIRTY             = 1 <<  6;
            /// Set this bit if this page table entry represents a "huge" page. 
            /// This bit may be used as follows:
            /// * For a P4-level PTE, it must be not set. 
            /// * If set for a P3-level PTE, it means this PTE maps a 1GiB huge page.
            /// * If set for a P2-level PTE, it means this PTE maps a 1MiB huge page.
            /// * For a P1-level PTE, it must be not set. 
            const HUGE_PAGE         = 1 <<  7;
            /// Set this bit to indicate that this page is mapped across all address spaces 
            /// (all root page tables) and doesn't need to be flushed out of the TLB 
            /// when switching to another page table.
            const GLOBAL            = 1 <<  8;

            /// Set this bit to indicate that the frame pointed to by this page table entry
            /// is owned **exclusively** by that page table entry.
            /// Currently, in Theseus, we only set the `EXCLUSIVE` bit for P1-level PTEs
            /// that we **know** are bijective (1-to-1 virtual-to-physical) mappings. 
            /// If this bit is set, the pointed frame will be safely deallocated
            /// once this page table entry is unmapped. 
            const EXCLUSIVE         = 1 <<  9;

            /// Set this bit to forbid execution of the mapped page.
            /// In other words, if you want the page to be executable, do NOT set this bit. 
            const NO_EXECUTE        = 1 << 63;
        }
    }

    type Arch = ArchSpecificPteFlags;

    impl From<PteFlags> for Arch {
        fn from(sw: PteFlags) -> Self {
            let mut hw = Self::empty();

            hw |= match sw.contains(PteFlags::VALID_ENTRY) {
                true => Self::PRESENT,
                false => Self::empty(),
            };

            hw |= match sw.contains(PteFlags::WRITABLE) {
                true => Self::WRITABLE,
                false => Self::empty(),
            };

            hw |= match sw.contains(PteFlags::EXECUTABLE) {
                false => Self::NO_EXECUTE,
                true => Self::empty(),
            };

            hw |= match sw.contains(PteFlags::CACHEABLE) {
                false => Self::NO_CACHE,
                true => Self::empty(),
            };

            hw |= match sw.contains(PteFlags::GLOBAL) {
                true => Self::GLOBAL,
                false => Self::empty(),
            };

            hw |= match sw.contains(PteFlags::EXCLUSIVE) {
                true => Self::EXCLUSIVE,
                false => Self::empty(),
            };

            hw
        }
    }

    impl From<Arch> for PteFlags {
        fn from(hw: Arch) -> Self {
            let mut sw = Self::empty();

            sw |= match hw.contains(Arch::PRESENT) {
                true => Self::VALID_ENTRY,
                false => Self::empty(),
            };

            sw |= match hw.contains(Arch::WRITABLE) {
                true => Self::WRITABLE,
                false => Self::empty(),
            };

            sw |= match hw.contains(Arch::NO_EXECUTE) {
                false => Self::EXECUTABLE,
                true => Self::empty(),
            };

            sw |= match hw.contains(Arch::NO_CACHE) {
                false => Self::CACHEABLE,
                true => Self::empty(),
            };

            sw |= match hw.contains(Arch::GLOBAL) {
                true => Self::GLOBAL,
                false => Self::empty(),
            };

            sw |= match hw.contains(Arch::EXCLUSIVE) {
                true => Self::EXCLUSIVE,
                false => Self::empty(),
            };

            sw
        }
    }
}
