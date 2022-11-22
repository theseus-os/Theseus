//! The x86_64-specific definitions of PTE flags.

use crate::PteFlags;
use bitflags::bitflags;

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
        const _USER_ACCESSIBLE    = 1 << 2;
        /// * If set, writes to this page go directly to memory.
        /// * It not set, writes are first written to the CPU cache, and then written to memory.
        ///   This is also known as "write-back".
        const _WRITE_THROUGH      = 1 << 3;
        /// * If set, this page's content is never cached, neither for read nor writes. 
        /// * If not set, this page's content is cached as normal, both for read nor writes. 
        const NO_CACHE           = 1 << 4;
        /// An alias for `NO_CACHE` in order to ease compatibility with aarch64.
        const DEVICE_MEMORY      = Self::NO_CACHE.bits;
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
        ///   * For a P1-level PTE, it must be not set. 
        /// * If not set, this is a normal 4KiB page mapping.
        const HUGE_PAGE          = 1 << 7;
        /// * If set, this page is mapped identically across all address spaces
        ///   (all root page tables) and doesn't need to be flushed out of the TLB
        ///   when switching to another address space (page table).
        /// * If not set, this page is mapped into only one or less than all address spaces,
        ///   or is mapped differently across different address spaces,
        ///   and thus be flushed out of the TLB when switching address spaces (page tables).
        ///
        /// Note: Theseus is a single address space system, so this flag makes no difference.
        const _GLOBAL             = 1 << 8;

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

impl PteFlagsX86_64 {
    /// Returns a new `PteFlagsX86_64` with the default value, in which
    /// only the `NOT_EXECUTABLE` bit is set.
    pub const fn new() -> Self {
        Self::NOT_EXECUTABLE
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
