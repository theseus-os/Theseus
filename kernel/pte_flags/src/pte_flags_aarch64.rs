//! The aarch64-specific definitions of PTE flags.
//!
//! The definition of these flags assumed that the [MAIR] index 0
//! has a "DEVICE nGnRE" entry, and [MAIR] index 1 has a Normal + Outer Shareable entry.
//!
//! [MAIR]: https://docs.rs/cortex-a/latest/cortex_a/registers/MAIR_EL1/index.html

use crate::PteFlags;
use bitflags::bitflags;
use static_assertions::const_assert_eq;

/// A mask for the bits of a page table entry that contain the physical frame address.
pub const PTE_FRAME_MASK: u64 = 0x000_7FFFFFFFFF_000;

// Ensure that we never expose reserved bits [12:50] as part of the ` interface.
const_assert_eq!(PteFlagsAarch64::all().bits() & PTE_FRAME_MASK, 0);


bitflags! {
    /// Page table entry (PTE) flags on aarch64.
    ///
    /// **Note:** items beginning with an underscore `_` are not used in Theseus.
    ///
    /// The designation of bits in each `PageTableEntry` is as such:
    /// * Bits `[0:11]` (inclusive) are reserved by hardware for access flags, cacheability flags,
    ///   shareability flags, and TLB storage flags.
    /// * Bits `[12:50]` (inclusive) are reserved by hardware to hold the physical frame address.
    /// * Bits `[51:54]` (inclusive) are reserved by hardware for more access flags.
    /// * Bits `[55:58]` (inclusive) are available for custom OS usage.
    /// * Bits `[59:63]` (inclusive) are reserved by hardware for extended access flags.
    #[doc(cfg(target_arch = "aarch64"))]
    pub struct PteFlagsAarch64: u64 {
        /// * If set, this page is currently "present" in memory. 
        /// * If not set, this page is not in memory, which could mean one of several things:
        ///   * The page is not mapped at all
        ///   * The page has been temporarily paged/swapped to disk
        ///   * The page is waiting to be mapped, i.e., for demand paging.
        const VALID              = 1 << 0;

        /// * If set, this represents a page descriptor.
        /// * If not set, this represents a block descriptor.
        const PAGE_DESCRIPTOR    = 1 << 1;

        /// Indicates the page's cacheability is described by MAIR Index 0.
        /// Theseus uses this index for "normal" memory.
        const MAIR_INDEX_0       = 0 << 2;
        /// This page maps device memory, i.e., memory-mapped I/O registers.
        /// Theseus uses `MAIR_INDEX_0` for this type of memory.
        const DEVICE_MEMORY      = Self::MAIR_INDEX_0.bits;
        /// Indicates the page's cacheability is described by MAIR Index 1.
        /// Theseus uses this index for "device" memory.
        const MAIR_INDEX_1       = 1 << 2;
        /// This page maps "normal" memory, i.e., non-device memory.
        /// Theseus uses `MAIR_INDEX_1` for this type of memory.
        const NORMAL_MEMORY      = Self::MAIR_INDEX_1.bits;
        /// Indicates the page's cacheability is described by MAIR Index 2.
        /// This is unused in Theseus.
        const _MAIR_INDEX_2      = 2 << 2;
        /// Indicates the page's cacheability is described by MAIR Index 3.
        /// This is unused in Theseus.
        const _MAIR_INDEX_3      = 3 << 2;
        /// Indicates the page's cacheability is described by MAIR Index 4.
        /// This is unused in Theseus.
        const _MAIR_INDEX_4      = 4 << 2;
        /// Indicates the page's cacheability is described by MAIR Index 5.
        /// This is unused in Theseus.
        const _MAIR_INDEX_5      = 5 << 2;
        /// Indicates the page's cacheability is described by MAIR Index 6.
        /// This is unused in Theseus.
        const _MAIR_INDEX_6      = 6 << 2;
        /// Indicates the page's cacheability is described by MAIR Index 7.
        /// This is unused in Theseus.
        const _MAIR_INDEX_7      = 7 << 2;

        /// * If set, this page is accessible in both Secure and Non-Secure execution levels.
        /// * If not set, this page is accessible in only Secure execution levels.
        /// 
        /// This is unused in Theseus.
        const _NON_SECURE_ACCESS = 1 << 5;

        /// * If set, userspace (unprivileged mode) can access this page.
        /// * If not set, only kernelspace (privileged mode) can access this page.
        const _USER_ACCESSIBLE   = 1 << 6;

        /// * If set, this page is read-only.
        /// * If not set, this page is writable.
        const READ_ONLY          = 1 << 7;

        /// Indicates that only a single CPU core may access this page.
        const _NON_SHAREABLE     = 0 << 8;
        // Shareable `0b01` is reserved.
        // const SHAREABLE_RSVD  = 1 << 8;
        /// Indicates that multiple CPUs from multiple clusters may access this page.
        /// This is the default and the the only value used in Theseus (and most systems).
        const OUTER_SHAREABLE    = 2 << 8;
        /// Multiple cores from the same
        /// cluster can access this page.
        /// Indicates that multiple CPUs from only a single cluster may access this page.
        const _INNER_SHAREABLE   = 3 << 8;

        /// * The hardware will set this bit when the page is accessed.
        /// * The OS can then clear this bit once it has acknowledged that the page was accessed,
        ///   if it cares at all about this information.
        /// 
        /// On aarch64, an "Access Flag Fault" may be raised if this bit is not set
        /// when this page is first accessed and is trying to be cached in the TLB.
        /// This fault can only occur when the Access Flag bit is `0` and the flag is being
        /// managed by software.
        const ACCESSED           = 1 << 10;
        /// * If set, this page is mapped into only one or less than all address spaces,
        ///   or is mapped differently across different address spaces,
        ///   and thus be flushed out of the TLB when switching address spaces (page tables).
        /// * If not set, this page is mapped identically across all address spaces
        ///   (all root page tables) and doesn't need to be flushed out of the TLB
        ///   when switching to another address space (page table).
        ///
        /// Note: Theseus is a single address space system, so this flag makes no difference.
        const _NOT_GLOBAL         = 1 << 11;

        /// * The hardware will set this bit when the page has been written to.
        /// * The OS can then clear this bit once it has acknowledged that the page was written to,
        ///   which is primarily useful for paging/swapping to disk.
        const DIRTY              = 1 << 51;
        /// * If set, this translation table is contiguous with the previous one in memory.
        /// * If not set, this translation table is not contiguous with the previous one in memory.
        /// 
        /// This is currently not used in Theseus.
        const _CONTIGUOUS         = 1 << 52;

        /// * If set, this page is not executable by privileged levels (kernel).
        /// * If not set, this page is executable by privileged levels (kernel).
        const PRIV_EXEC_NEVER    = 1 << 53;
        /// * If set, this page is not executable by unprivileged levels (user).
        /// * If not set, this page is executable by unprivileged levels (user).
        const USER_EXEC_NEVER    = 1 << 54;
        const NOT_EXECUTABLE     = Self::PRIV_EXEC_NEVER.bits | Self::USER_EXEC_NEVER.bits;

        /// See [PteFlags::EXCLUSIVE].
        ///  We use bit 55 because it is available for custom OS usage on both x86_64 and aarch64.
        const EXCLUSIVE          = 1 << 55;
    }
}

/// See [`PteFlagsAarch64::new()`] for what bits are set by default.
impl Default for PteFlagsAarch64 {
    fn default() -> Self {
        Self::new()
    }
}

impl PteFlagsAarch64 {
    /// The mask of bits that should be overwritten with default values
    /// when converting a generic `PteFlags` into a specific `PteFlagsAarch64`.
    /// Currently this includes:
    /// * The two bits `[8:9]` for shareability.
    pub const OVERWRITTEN_BITS_FOR_CONVERSION: PteFlagsAarch64 =
        PteFlagsAarch64::_INNER_SHAREABLE;


    /// Returns a new `PteFlagsAarch64` with the default value, in which:
    /// * `NORMAL_MEMORY` (not `DEVICE_MEMORY`) is set.
    /// * `OUTER_SHAREABLE` is set.
    /// * `READ_ONLY` is set.
    /// * `ACCESSED` is set.
    /// * `NOT_GLOBAL` is set.
    /// * the `NOT_EXECUTABLE` bits are set.
    ///
    /// Note: the `ACCESSED` bit is set by default because Theseus 
    ///       currently doesn't perform any paging/swapping of pages to disk,
    ///       which is what this bit is typically used for.
    ///       On aarch64, not setting this bit can cause an Access Flag Fault
    ///       (which is useful only for software-managed LRU paging algorithms),
    ///       so we just set that bit by default to avoid any faults
    ///       that we don't care about.
    pub const fn new() -> Self {
        Self::from_bits_truncate(
            Self::NORMAL_MEMORY.bits
            | Self::OUTER_SHAREABLE.bits
            | Self::READ_ONLY.bits
            | Self::ACCESSED.bits
            | Self::_NOT_GLOBAL.bits
            | Self::NOT_EXECUTABLE.bits
        )
    }
}

impl From<PteFlags> for PteFlagsAarch64 {
    /// When converting from `PteFlags` to `PteFlagsAarch64`, the bits given by
    /// [`PteFlagsAarch64::OVERWRITTEN_BITS_FOR_CONVERSION`] will be overwritten
    /// with a default value.
    /// 
    /// Currently, this includes:
    /// * `OUTER_SHAREABLE` will be set.
    fn from(general: PteFlags) -> Self {
        let mut specific = Self::from_bits_truncate(general.bits());
        specific.toggle(super::WRITABLE_BIT | super::GLOBAL_BIT);
        specific &= !Self::OVERWRITTEN_BITS_FOR_CONVERSION; // clear the masked bits
        specific |= Self::OUTER_SHAREABLE; // set the masked bits to their default
        specific
    }
}

impl From<PteFlagsAarch64> for PteFlags {
    fn from(mut specific: PteFlagsAarch64) -> Self {
        specific.toggle(super::WRITABLE_BIT | super::GLOBAL_BIT);
        Self::from_bits_truncate(specific.bits())
    }
}
