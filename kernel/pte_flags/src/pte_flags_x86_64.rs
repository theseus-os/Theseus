//! The x86_64-specific definitions of PTE flags.

use crate::PteFlags;
use bitflags::bitflags;

/// A mask for the bits of a page table entry that contain the physical frame address.
pub const PTE_FRAME_MASK: u64 = 0x000F_FFFF_FFFF_F000;

// Ensure that we never expose reserved bits [12:51] as part of the `PteFlagsX86_64` interface.
const _: () = assert!(PteFlagsX86_64::all().bits() & PTE_FRAME_MASK == 0);


bitflags! {
    /// Page table entry (PTE) flags on x86_64.
    ///
    /// **Note:** items beginning with an underscore `_` are not used in Theseus.
    ///
    /// The designation of bits in each `PageTableEntry` is as such:
    /// * Bits `[0:8]` (inclusive) are reserved by hardware for access flags.
    /// * Bits `[9:11]` (inclusive) are available for custom OS usage.
    /// * Bits `[12:51]` (inclusive) are reserved by hardware to hold the physical frame address.
    /// * Bits `[52:62]` (inclusive) are available for custom OS usage.
    /// * Bit  `63` is reserved by hardware for access flags (noexec).
    #[doc(cfg(target_arch = "x86_64"))]
    pub struct PteFlagsX86_64: u64 {
        /// * If set, this page is currently "present" in memory. 
        /// * If not set, this page is not in memory, which could mean one of several things:
        ///   * The page is not mapped at all
        ///   * The page has been temporarily paged/swapped to disk
        ///   * The page is waiting to be mapped, i.e., for demand paging.
        const VALID              = 1 << 0;

        /// * If set, this page is writable.
        /// * If not set, this page is read-only.
        const WRITABLE           = 1 << 1;

        /// * If set, userspace (ring 3) can access this page.
        /// * If not set, only kernelspace (ring 0) can access this page.
        ///
        /// This is unused in Theseus because it is a single privilege level OS.
        const _USER_ACCESSIBLE   = 1 << 2;

        /// * If set, writes to this page go directly to memory.
        /// * It not set, writes are first written to the CPU cache, and then written to memory.
        ///   This is also known as "write-back".
        ///
        /// If the Page Attribute Table (PAT) feature is enabled, this represents
        /// the least-significant bit of the 3-bit index into the Page Attribute Table;
        /// that index is used to determine the PAT entry that holds the
        /// memory caching type that is applied to this page.
        const WRITE_THROUGH      = 1 << 3;
        const PAT_BIT0           = Self::WRITE_THROUGH.bits;

        /// * If set, this page's content is never cached, neither for read nor writes. 
        /// * If not set, this page's content is cached as normal, both for read nor writes. 
        ///
        /// If the Page Attribute Table (PAT) feature is enabled, this represents
        /// the middle bit of the 3-bit index into the Page Attribute Table;
        /// that index is used to determine the PAT entry that holds the
        /// memory caching type that is applied to this page.
        const CACHE_DISABLE      = 1 << 4;
        /// An alias for [`Self::CACHE_DISABLE`] in order to ease compatibility with aarch64.
        const DEVICE_MEMORY      = Self::CACHE_DISABLE.bits;
        const PAT_BIT1           = Self::CACHE_DISABLE.bits;

        /// * The hardware will set this bit when the page is accessed.
        /// * The OS can then clear this bit once it has acknowledged that the page was accessed,
        ///   if it cares at all about this information.
        const ACCESSED           = 1 << 5;

        /// * The hardware will set this bit when the page has been written to.
        /// * The OS can then clear this bit once it has acknowledged that the page was written to,
        ///   which is primarily useful for paging/swapping to disk.
        const DIRTY              = 1 << 6;

        /// * If set, this page table entry represents a "huge" page. 
        ///   This bit may be used as follows:
        ///   * For a P4-level PTE, it must be not set. 
        ///   * If set for a P3-level PTE, it means this PTE maps a 1GiB huge page.
        ///   * If set for a P2-level PTE, it means this PTE maps a 1MiB huge page.
        ///   * A P1-level PTE cannot map a huge page, so this bit is interpreted
        ///     as [`Self::PAT_FOR_P1`] instead.
        /// * If not set, this is a normal 4KiB page mapping.
        const HUGE_PAGE          = 1 << 7;
        /// (For P1-level (lowest level) page tables ONLY):
        /// If the Page Attribute Table (PAT) feature is enabled, this represents
        /// the most-significant bit of the 3-bit index into the Page Attribute Table;
        /// that index is used to determine the PAT entry that holds the
        /// memory caching type that is applied to this page.
        /// 
        /// This *cannot* be used for PAT index bits in a mid-level (P2 or P3) entry.
        const PAT_BIT2_FOR_P1    = 1 <<  7;

        /// * If set, this page is mapped identically across all address spaces
        ///   (all root page tables) and doesn't need to be flushed out of the TLB
        ///   when switching to another address space (page table).
        /// * If not set, this page is mapped into only one or less than all address spaces,
        ///   or is mapped differently across different address spaces,
        ///   and thus be flushed out of the TLB when switching address spaces (page tables).
        ///
        /// Note: Theseus is a single address space system, so this flag makes no difference.
        const _GLOBAL            = 1 <<  8;

        // Note: Theseus currently only supports setting PAT bits for P1-level PTEs.
        //
        // /// (For P2- and P3- level (mid-level) page tables ONLY):
        // /// If the Page Attribute Table (PAT) feature is enabled, this represents
        // /// the most-significant bit of the 3-bit index into the Page Attribute Table;
        // /// that index is used to determine the PAT entry that holds the
        // /// memory caching type that is applied to this page.
        // /// 
        // /// This *cannot* be used for PAT index bits in a lowest-level (P1) PTE.
        // const PAT_BIT2_FOR_P2_P3 = 1 << 12;

        /// See [PteFlags::EXCLUSIVE].
        ///  We use bit 55 because it is available for custom OS usage on both x86_64 and aarch64.
        const EXCLUSIVE          = 1 << 55;

        /// * If set, this page is not executable.
        /// * If not set, this page is executable.
        const NOT_EXECUTABLE     = 1 << 63;
    }
}

/// See [`PteFlagsX86_64::new()`] for what bits are set by default.
impl Default for PteFlagsX86_64 {
    fn default() -> Self {
        Self::new()
    }
}

/// Functions common to PTE flags on all architectures.
impl PteFlagsX86_64 {
    /// Returns a new `PteFlagsX86_64` with the default value, in which
    /// only the `NOT_EXECUTABLE` bit is set.
    pub const fn new() -> Self {
        Self::NOT_EXECUTABLE
    }

    /// Returns a copy of this `PteFlagsX86_64` with the `VALID` bit set or cleared.
    ///
    /// * If `enable` is `true`, this PTE will be considered "present" and "valid",
    ///   meaning that the mapping from this page to a physical frame is valid
    ///   and that the translation of a virtual address in this page should succeed.
    /// * If `enable` is `false`, this PTE will be considered "invalid",
    ///   and any attempt to access it for translation purposes will cause a page fault.
    #[must_use]
    #[doc(alias("present"))]
    pub fn valid(mut self, enable: bool) -> Self {
        self.set(Self::VALID, enable);
        self
    }

    /// Returns a copy of this `PteFlagsX86_64` with the `WRITABLE` bit set or cleared.
    ///
    /// * If `enable` is `true`, this will be writable.
    /// * If `enable` is `false`, this will be read-only.
    #[must_use]
    #[doc(alias("read_only"))]
    pub fn writable(mut self, enable: bool) -> Self {
        self.set(Self::WRITABLE, enable);
        self
    }

    /// Returns a copy of this `PteFlagsX86_64` with the `NOT_EXECUTABLE` bit cleared or set.
    ///
    /// * If `enable` is `true`, this page will be executable (`NOT_EXECUTABLE` will be cleared).
    /// * If `enable` is `false`, this page will be non-executable, which is the default
    ///   (`NOT_EXECUTABLE` will be set).
    #[must_use]
    #[doc(alias("no_exec"))]
    pub fn executable(mut self, enable: bool) -> Self {
        self.set(Self::NOT_EXECUTABLE, !enable);
        self
    }

    /// Returns a copy of this `PteFlagsX86_64` with the `DEVICE_MEMORY` bit set or cleared.
    ///
    /// * If `enable` is `true`, this will be non-cacheable device memory.
    /// * If `enable` is `false`, this will be "normal" memory, the default.
    #[must_use]
    #[doc(alias("cache", "cacheable", "non-cacheable"))]
    pub fn device_memory(mut self, enable: bool) -> Self {
        self.set(Self::DEVICE_MEMORY, enable);
        self
    }

    /// Returns a copy of this `PteFlagsX86_64` with the `EXCLUSIVE` bit set or cleared.
    ///
    /// * If `enable` is `true`, this page will exclusively map its frame.
    /// * If `enable` is `false`, this page will NOT exclusively map its frame.
    #[must_use]
    pub fn exclusive(mut self, enable: bool) -> Self {
        self.set(Self::EXCLUSIVE, enable);
        self
    }

    /// Returns a copy of this `PteFlagsX86_64` with the `ACCESSED` bit set or cleared.
    ///
    /// Typically this is used to clear the `ACCESSED` bit, in order to indicate
    /// that the OS has "acknowledged" the fact that this page was accessed
    /// since the last time it checked.
    ///
    /// * If `enable` is `true`, this page will be marked as accessed.
    /// * If `enable` is `false`, this page will be marked as not accessed.
    #[must_use]
    pub fn accessed(mut self, enable: bool) -> Self {
        self.set(Self::ACCESSED, enable);
        self
    }

    /// Returns a copy of this `PteFlagsX86_64` with the `DIRTY` bit set or cleared.
    ///
    /// Typically this is used to clear the `DIRTY` bit, in order to indicate
    /// that the OS has "acknowledged" the fact that this page was written to
    /// since the last time it checked. 
    /// This bit is typically set by the hardware.
    ///
    /// * If `enable` is `true`, this page will be marked as dirty.
    /// * If `enable` is `false`, this page will be marked as clean.
    #[must_use]
    pub fn dirty(mut self, enable: bool) -> Self {
        self.set(Self::DIRTY, enable);
        self
    }

    #[doc(alias("present"))]
    pub const fn is_valid(&self) -> bool {
        self.contains(Self::VALID)
    }

    #[doc(alias("read_only"))]
    pub const fn is_writable(&self) -> bool {
        self.contains(Self::WRITABLE)
    }

    #[doc(alias("no_exec"))]
    pub const fn is_executable(&self) -> bool {
        !self.contains(Self::NOT_EXECUTABLE)
    }

    #[doc(alias("cache", "cacheable", "non-cacheable"))]
    pub const fn is_device_memory(&self) -> bool {
        self.contains(Self::DEVICE_MEMORY)
    }

    pub const fn is_dirty(&self) -> bool {
        self.contains(Self::DIRTY)
    }

    pub const fn is_accessed(&self) -> bool {
        self.contains(Self::ACCESSED)
    }

    pub const fn is_exclusive(&self) -> bool {
        self.contains(Self::EXCLUSIVE)
    }
}

const BIT_0: u8 = 1 << 0;
const BIT_1: u8 = 1 << 1;
const BIT_2: u8 = 1 << 2;

/// Functions specific to x86_64 PTE flags only.
impl PteFlagsX86_64 {
    /// Returns a copy of this `PteFlagsX86_64` with its flags adjusted
    /// for use in a higher-level page table entry, e.g., P4, P3, P2.
    ///
    /// Currently, on x86_64, this does the following:
    /// * Clears the `NOT_EXECUTABLE` bit.  
    ///   * P4, P3, and P2 entries should never set `NOT_EXECUTABLE`,
    ///     only the lowest-level P1 entry should.
    /// * Clears the `EXCLUSIVE` bit.
    ///   * Currently, we do not use the `EXCLUSIVE` bit for P4, P3, or P2 entries,
    ///     because another page table frame may re-use it (create another alias to it)
    ///     without our page table implementation knowing about it.
    ///   * Only P1-level PTEs can map a frame exclusively.
    /// * Clears the PAT index value, as we only support PAT on P1-level PTEs.
    /// * Sets the `VALID` bit, as every P4, P3, and P2 entry must be valid.
    #[must_use]
    pub fn adjust_for_higher_level_pte(self) -> Self {
        self.executable(true)
            .exclusive(false)
            .pat_index(0)
            .valid(true)
    }

    /// Returns a copy of this `PteFlagsX86_64` with the PAT index bits
    /// set to the value specifying the given `pat_slot`.
    ///
    /// This sets the following bits:
    /// * [`PteFlagsX86_64::PAT_BIT0`] = Bit 0 of `pat_slot`
    /// * [`PteFlagsX86_64::PAT_BIT1`] = Bit 1 of `pat_slot`
    /// * [`PteFlagsX86_64::PAT_BIT2_FOR_P1`] = Bit 2 of `pat_slot`
    ///
    /// The other bits `[3:7]` of `pat_slot` are ignored.
    #[must_use]
    #[doc(alias("PAT", "page attribute table", "slot"))]
    pub fn pat_index(mut self, pat_slot: u8) -> Self {
        self.set(Self::PAT_BIT0,        pat_slot & BIT_0 == BIT_0);
        self.set(Self::PAT_BIT1,        pat_slot & BIT_1 == BIT_1);
        self.set(Self::PAT_BIT2_FOR_P1, pat_slot & BIT_2 == BIT_2);
        self
    }

    #[doc(alias("PAT", "page attribute table", "slot"))]
    pub fn get_pat_index(&self) -> u8 {
        let mut pat_index = 0;
        if self.contains(Self::PAT_BIT0)        { pat_index |= BIT_0; }
        if self.contains(Self::PAT_BIT1)        { pat_index |= BIT_1; }
        if self.contains(Self::PAT_BIT2_FOR_P1) { pat_index |= BIT_2; }
        pat_index
    }

    pub const fn is_huge(&self) -> bool {
        self.contains(Self::HUGE_PAGE)
    }

}

impl From<PteFlags> for PteFlagsX86_64 {
    fn from(general: PteFlags) -> Self {
        Self::from_bits_truncate(general.bits())
    }
}

impl From<PteFlagsX86_64> for PteFlags {
    fn from(specific: PteFlagsX86_64) -> Self {
        Self::from_bits_truncate(specific.bits())
    }
}
