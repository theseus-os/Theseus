//! The aarch64-specific definitions of PTE flags.

use crate::PteFlags;
use bitflags::bitflags;
use static_assertions::const_assert_eq;

/// A mask for the bits of a page table entry that contain the physical frame address.
pub const PTE_FRAME_MASK: u64 = 0x0000_FFFF_FFFF_F000;

// Ensure that we never expose reserved bits [12:47] as part of the ` interface.
const_assert_eq!(PteFlagsAarch64::all().bits() & PTE_FRAME_MASK, 0);


bitflags! {
    /// Page table entry (PTE) flags on aarch64.
    ///
    /// **Note:** items beginning with an underscore `_` are not used in Theseus.
    ///
    /// The designation of bits in each `PageTableEntry` is as such:
    /// * Bits `[0:11]` (inclusive) are reserved by hardware for access flags, cacheability flags,
    ///   shareability flags, and TLB storage flags.
    /// * Bits `[12:47]` (inclusive) are reserved by hardware to hold the physical frame address.
    /// * Bits `[48:49]` (inclusive) are reserved as zero.
    /// * Bits `[50:54]` (inclusive) are reserved by hardware for more access flags.
    /// * Bits `[55:58]` (inclusive) are available for custom OS usage.
    /// * Bits `[59:63]` (inclusive) are reserved by hardware for extended access flags.
    ///
    ///
    /// ## Assumed System Configuration
    /// * The system has been configured to use 48-bit physical addresses
    ///   (aka "OA"s: Output Addresses).
    /// * The system has been configured to use only a single translation stage, Stage 1.
    /// * The [MAIR] index 0 has a Normal + Outer Shareable entry.
    /// * The [MAIR] index 1 has a "DEVICE nGnRE" entry.
    ///
    /// [MAIR]: https://docs.rs/cortex-a/latest/cortex_a/registers/MAIR_EL1/index.html
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
        ///
        /// Theseus uses this index for "normal" memory.
        const _MAIR_INDEX_0      = 0 << 2;
        /// This page maps "normal" memory, i.e., non-device memory.
        ///
        /// Theseus uses `MAIR_INDEX_0` for this type of memory.
        const NORMAL_MEMORY      = Self::_MAIR_INDEX_0.bits;
        /// Indicates the page's cacheability is described by MAIR Index 1.
        ///
        /// Theseus uses this index for "device" memory.
        const _MAIR_INDEX_1      = 1 << 2;
        /// This page maps device memory, i.e., memory-mapped I/O registers.
        ///
        /// Theseus uses `MAIR_INDEX_1` for this type of memory.
        const DEVICE_MEMORY      = Self::_MAIR_INDEX_1.bits;
        /// Indicates the page's cacheability is described by MAIR Index 2.
        ///
        /// This is unused in Theseus.
        const _MAIR_INDEX_2      = 2 << 2;
        /// Indicates the page's cacheability is described by MAIR Index 3.
        ///
        /// This is unused in Theseus.
        const _MAIR_INDEX_3      = 3 << 2;
        /// Indicates the page's cacheability is described by MAIR Index 4.
        ///
        /// This is unused in Theseus.
        const _MAIR_INDEX_4      = 4 << 2;
        /// Indicates the page's cacheability is described by MAIR Index 5.
        ///
        /// This is unused in Theseus.
        const _MAIR_INDEX_5      = 5 << 2;
        /// Indicates the page's cacheability is described by MAIR Index 6.
        ///
        /// This is unused in Theseus.
        const _MAIR_INDEX_6      = 6 << 2;
        /// Indicates the page's cacheability is described by MAIR Index 7.
        ///
        /// This is unused in Theseus.
        const _MAIR_INDEX_7      = 7 << 2;

        /// * If set, this page is accessible in both Secure and Non-Secure execution levels.
        /// * If not set, this page is accessible in only Secure execution levels.
        /// 
        /// This is unused in Theseus.
        const _NON_SECURE_ACCESS = 1 << 5;

        /// * If set, userspace (unprivileged mode) can access this page.
        /// * If not set, only kernelspace (privileged mode) can access this page.
        ///
        /// This is unused in Theseus because it is a single privilege level OS.
        const _USER_ACCESSIBLE   = 1 << 6;

        /// * If set, this page is read-only.
        /// * If not set, this page is writable.
        const READ_ONLY          = 1 << 7;

        /// Indicates that only a single CPU core may access this page.
        ///
        /// This is not used and not supported by Theseus; use [`Self::OUTER_SHAREABLE`].
        const _NON_SHAREABLE     = 0 << 8;
        // Shareable `0b01` is reserved.
        // const SHAREABLE_RSVD  = 1 << 8;
        /// Indicates that multiple CPUs from multiple clusters may access this page.
        ///
        /// This is the default and the the only value used in Theseus (and most systems).
        const OUTER_SHAREABLE    = 2 << 8;
        /// Multiple cores from the same
        /// cluster can access this page.
        /// Indicates that multiple CPUs from only a single cluster may access this page.
        ///
        /// This is not used and not supported by Theseus; use [`Self::OUTER_SHAREABLE`].
        const _INNER_SHAREABLE   = 3 << 8;

        /// * The hardware will set this bit when the page is accessed.
        /// * The OS can then clear this bit once it has acknowledged that the page was accessed,
        ///   if it cares at all about this information.
        /// 
        /// On aarch64, an "Access Flag Fault" may be raised if this bit is not set
        /// when this page is first accessed and is trying to be cached in the TLB.
        /// This fault can only occur when the Access Flag bit is `0` and the flag is being
        /// managed by software.
        ///
        /// Thus, Theseus currently *always* sets this bit by default.
        const ACCESSED           = 1 << 10;
        /// * If set, this page is mapped into only one or less than all address spaces,
        ///   or is mapped differently across different address spaces,
        ///   and thus be flushed out of the TLB when switching address spaces (page tables).
        /// * If not set, this page is mapped identically across all address spaces
        ///   (all root page tables) and doesn't need to be flushed out of the TLB
        ///   when switching to another address space (page table).
        ///
        /// Note: Theseus is a single address space system, so this flag makes no difference.
        const _NOT_GLOBAL        = 1 << 11;

        /// * If set, this page is considered a "Guarded Page",
        ///   which can be used to protect against executing instructions
        ///   that aren't the intended target of a branch (e.g., with `BTI` instruction).
        /// 
        /// This is only available if `FEAT_BTI` is implemented;
        /// otherwise it is reserved as 0.
        ///
        /// This is currently not used in Theseus.
        const _GUARDED_PAGE      = 1 << 50;
        /// * The hardware will set this bit when the page has been written to.
        /// * The OS can then clear this bit once it has acknowledged that the page was written to,
        ///   which is primarily useful for paging/swapping to disk.
        const DIRTY              = 1 << 51;
        /// * If set, this translation table entry is part of a set that is contiguous in memory
        ///   with adjacent entries that also have this bit set.
        /// * If not set, this translation table entry is not contiguous in memory
        ///   with entries that are adjancent to it.
        ///
        /// This is useful for reducing TLB pressure because the TLB entries for
        /// multiple contiguous adjacent entries can be combined into one TLB entry.
        ///
        /// This is currently not used in Theseus.
        const _CONTIGUOUS        = 1 << 52;

        /// * If set, this page is not executable by privileged levels (kernel).
        /// * If not set, this page is executable by privileged levels (kernel).
        ///
        /// In Theseus, use [`Self::NOT_EXECUTABLE`] instead.
        const _PRIV_EXEC_NEVER   = 1 << 53;
        /// * If set, this page is not executable by unprivileged levels (user).
        /// * If not set, this page is executable by unprivileged levels (user).
        ///
        /// In Theseus, use [`Self::NOT_EXECUTABLE`] instead.
        const _USER_EXEC_NEVER   = 1 << 54;
        /// * If set, this page is not executable.
        /// * If not set, this page is executable.
        const NOT_EXECUTABLE     = Self::_PRIV_EXEC_NEVER.bits | Self::_USER_EXEC_NEVER.bits;

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
    /// The mask of bit ranges that cannot be handled by toggling,
    /// as they are not single bit values, but multi-bit selectors/indices.
    ///
    /// Currently this includes:
    /// * The three bits `[2:4]` for MAIR index values.
    /// * The two bits `[8:9]` for shareability.
    pub const MASKED_BITS_FOR_CONVERSION: PteFlagsAarch64 = PteFlagsAarch64::from_bits_truncate(
        PteFlagsAarch64::_INNER_SHAREABLE.bits | PteFlagsAarch64::_MAIR_INDEX_7.bits
    );

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
    /// When converting from `PteFlags` to `PteFlagsAarch64`,
    /// some ranges of bits must be given a default value.
    /// 
    /// Currently, this includes:
    /// * `OUTER_SHAREABLE` will be set.
    fn from(general: PteFlags) -> Self {
        let mut specific = Self::from_bits_truncate(general.bits());
        // The writable and global bit values have inverse meanings on aarch64.
        specific.toggle(super::WRITABLE_BIT | super::GLOBAL_BIT);
        // Mask out the ranges of bits that can't simply be toggled; we must manually set them.
        specific &= !Self::MASKED_BITS_FOR_CONVERSION;
        specific |= Self::OUTER_SHAREABLE; // OUTER_SHAREABLE is the default value
        if general.contains(PteFlags::DEVICE_MEMORY) {
            specific |= Self::DEVICE_MEMORY;
        } else {
            specific |= Self::NORMAL_MEMORY;
        }
        specific
    }
}

impl From<PteFlagsAarch64> for PteFlags {
    fn from(mut specific: PteFlagsAarch64) -> Self {
        // The writable and global bit values have inverse meanings on aarch64.
        specific.toggle(super::WRITABLE_BIT | super::GLOBAL_BIT);
        let mut general = Self::from_bits_truncate(specific.bits());
        // Ensure that we are strict about which MAIR index is used by explicitly masking it.
        // Otherwise, `DEVICE_MEMORY` may accidentally be misinterpreted as enabled
        // if another MAIR index that had overlapping bits (bit 2) was specified,
        // e.g., _MAIR_INDEX_3, _MAIR_INDEX_5, or _MAIR_INDEX_7.
        if specific & PteFlagsAarch64::_MAIR_INDEX_7 == PteFlagsAarch64::DEVICE_MEMORY {
            general |= Self::DEVICE_MEMORY;
        }
        general
    }
}
