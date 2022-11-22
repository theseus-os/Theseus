//! This crate defines the structure of page table entry (PTE) flags on x86_64 and aarch64.
//! 
//! This crate offers two main types:
//! * [`PteFlags`]: the set of bit flags that apply to all architectures.
//! * [`PteFlagsArch`]: the arch-specific set of bit flags that apply to
//!   only x86_64 or aarch64, depending on build target.
//!   * On x86_64, this is a re-export of the `PteFlagsX86_64` type.
//!   * On aarch64, this is a re-export of the `PteFlagsAarch64` type.
//! 
//! ## Type conversions
//! *Notably*, you can convert to and from these architecture-specific types
//! and architecture-generic type easily.
//! `PteFlags` can be losslessly converted into `PteFlagsX86_64` or `PteFlagsAarch64`,
//! with the typical [`From`] and [`Into`] traits.
//! This makes it possible to set general architecture-indepedent flags first,
//! and then convert it in order to set more architecture-specific flags.
//! 
//! You can also convert `PteFlagsX86_64` or `PteFlagsAarch64` into `PteFlags`,
//! but it may be lossy as only the bit flags defined in `PteFlags` are preserved.
//! 
//! ## aarch64 considerations
//! When converting from `PteFlags` to `PteFlagsAarch64`,
//! certain bits will be set by default;
//! see [`PteFlagsAarch64::from()`] for more information.
//! 
//! For aarch64, the definition of these flags assumes that the [MAIR] index 0
//! has a "DEVICE nGnRE" entry, and [MAIR] Index 1 has a Normal + Outer Shareable entry.
//!
//! [MAIR]: https://docs.rs/cortex-a/latest/cortex_a/registers/MAIR_EL1/index.html

#![no_std]

use cfg_if::cfg_if;
use bitflags::bitflags;

cfg_if!{ if #[cfg(target_arch = "x86_64")] {
    mod pte_flags_x86_64;
    pub use pte_flags_x86_64::PteFlagsX86_64;
    /// A re-export of `PteFlagsX86_64` for convenience.
    pub use pte_flags_x86_64::PteFlagsX86_64 as PteFlagsArch;
} else if #[cfg(target_arch = "aarch64")] {
    mod pte_flags_aarch64;
    pub use pte_flags_aarch64::PteFlagsAarch64;
    /// A re-export of `PteFlagsAarch64` for convenience.
    pub use pte_flags_aarch64::PteFlagsAarch64 as PteFlagsArch;
}}

bitflags! {
    /// Common, architecture-independent flags for a page table entry (PTE)
    /// that define how a page is mapped.
    ///
    /// **Note:** items beginning with an underscore `_` are not used in Theseus.
    ///
    /// This contains only the flags that are common to both `x86_64` and `aarch64`.
    ///
    /// ## Converting to/from arch-specific flags
    /// This type can be losslessly converted into `PteFlagsX86_64` and `PteFlagsAarch64`
    /// with the typical [`From`] and [`Into`] traits.
    /// This makes it easier to set general architecture-indepedent flags first,
    /// and then convert it in order to set more architecture-specific flags.
    /// 
    /// This type can also be converted *from* `PteFlagsX86_64` and `PteFlagsAarch64`,
    /// but it may be lossy as only the bit flags defined herein are preserved.
    pub struct PteFlags: u64 {
        /// * If set, this page is currently "present" in memory. 
        /// * If not set, this page is not in memory, which could mean one of several things:
        ///   * The page is not mapped at all.
        ///   * The page has been temporarily paged/swapped to disk.
        ///   * The page is waiting to be mapped, i.e., for demand paging.
        //
        // This does not require a conversion between architectures.
        const VALID = PteFlagsArch::VALID.bits();

        /// * If set, this page is writable.
        /// * If not set, this page is read-only.
        //
        // This DOES require a conversion for aarch64, but not for x86_64.
        const WRITABLE = WRITABLE_BIT.bits();

        /// * If set, userspace (unprivileged mode) can access this page.
        /// * If not set, only kernelspace (privileged mode) can access this page.
        ///
        /// This is not used in Theseus, because it has a single privilege level.
        //
        // This does not require a conversion between architectures.
        const _USER_ACCESSIBLE = PteFlagsArch::_USER_ACCESSIBLE.bits();
        
        /// * If set, this page maps device memory, which is non-cacheable.
        /// * If not set, this page maps normal memory, which is cacheable by default.
        //
        // This does not require a conversion between architectures.
        const DEVICE_MEMORY  = PteFlagsArch::DEVICE_MEMORY.bits();
        
        /// * The hardware will set this bit when the page is accessed.
        /// * The OS can then clear this bit once it has acknowledged that the page was accessed,
        ///   if it cares at all about this information.
        //
        // This does not require a conversion between architectures.
        const ACCESSED = PteFlagsArch::ACCESSED.bits();

        /// * The hardware will set this bit when the page has been written to.
        /// * The OS can then clear this bit once it has acknowledged that the page was written to,
        ///   which is primarily useful for paging/swapping to disk.
        //
        // This does not require a conversion between architectures.
        const DIRTY = PteFlagsArch::DIRTY.bits();
        
        /// * If set, this page is mapped identically across all address spaces 
        ///   (all root page tables) and doesn't need to be flushed out of the TLB 
        ///   when switching to another address space (page table).
        /// * If not set, this page is mapped into only one or less than all address spaces,
        ///   or is mapped differently across different address spaces,
        ///   and thus be flushed out of the TLB when switching address spaces (page tables).
        ///
        /// This is not used in Theseus, as it has a single address space.
        //
        // This DOES require a conversion for aarch64, but not for x86_64.
        const _GLOBAL = GLOBAL_BIT.bits();

        /// * If set, this page is not executable.
        /// * If not set, this page is executable.
        //
        // This does not require a conversion between architectures.
        const NOT_EXECUTABLE = PteFlagsArch::NOT_EXECUTABLE.bits();

        /// Note: code that invokes memory management functions in Theseus cannot actually
        ///       set this flag. When flags are passed to those functions, 
        ///       this bit value is ignored and overridden as appropriate.
        /// 
        /// * If set, the frame mapped by this page table entry is owned **exclusively**
        ///   by that page table entry.
        ///   Currently, in Theseus, we only set the `EXCLUSIVE` bit for P1-level PTEs
        ///   that we **know** are bijective (1-to-1 virtual-to-physical) mappings. 
        ///   This allows Theseus to safely deallocate the frame mapped by this page
        ///   once this page table entry is unmapped. 
        /// * If not set, the frame mapped by this page is not owned exclusively
        ///   and thus cannot be safely deallocated when this page is unmapped.
        //
        // This does not require a conversion between architectures.
        const EXCLUSIVE = PteFlagsArch::EXCLUSIVE.bits();
    }
}

// The bits defined below have different semantics on x86_64 vs aarch64.
// These are the ones that require special handling during From/Into conversions.
cfg_if!{ if #[cfg(target_arch = "x86_64")] {
    const WRITABLE_BIT: PteFlagsX86_64 = PteFlagsX86_64::WRITABLE;
    const GLOBAL_BIT:   PteFlagsX86_64 = PteFlagsX86_64::_GLOBAL;
} else if #[cfg(target_arch = "aarch64")] {
    const WRITABLE_BIT: PteFlagsAarch64 = PteFlagsAarch64::READ_ONLY;
    const GLOBAL_BIT:   PteFlagsAarch64 = PteFlagsAarch64::_NOT_GLOBAL;
}}

/// See [`PteFlags::new()`] for what bits are set by default.
impl Default for PteFlags {
    fn default() -> Self {
        Self::new()
    }
}

impl PteFlags {
    /// Returns a new `PteFlagsX86_64` with the default value, in which:
    /// * `ACCESSED` is set.
    /// * the `NOT_EXECUTABLE` bit is set.
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
            Self::ACCESSED.bits
            | Self::NOT_EXECUTABLE.bits
        )
    }

    /// Returns a copy of this `PteFlags` with the `VALID` bit set or cleared.
    #[must_use]
    #[doc(alias("present"))]
    pub fn valid(mut self, enable: bool) -> Self {
        self.set(Self::VALID, enable);
        self
    }

    /// Returns a copy of this `PteFlags` with the `WRITABLE` bit set or cleared.
    ///
    /// * If `enable` is `true`, this will be writable.
    /// * If `enable` is `false`, this will be read-only.
    #[must_use]
    #[doc(alias("read_only"))]
    pub fn writable(mut self, enable: bool) -> Self {
        self.set(Self::WRITABLE, enable);
        self
    }

    /// Returns a copy of this `PteFlags` with the `NOT_EXECUTABLE` bit set or cleared.
    ///
    /// * If `enable` is `true`, this will be executable (`NOT_EXECUTABLE` will be cleared).
    /// * If `enable` is `false`, this will be non-executable, which is the default
    ///   (`NOT_EXECUTABLE` will be set).
    #[must_use]
    #[doc(alias("no_exec"))]
    pub fn executable(mut self, enable: bool) -> Self {
        self.set(Self::NOT_EXECUTABLE, !enable);
        self
    }

    /// Returns a copy of this `PteFlags` with the `DEVICE_MEMORY` bit set or cleared.
    ///
    /// * If `enable` is `true`, this will be non-cacheable device memory.
    /// * If `enable` is `false`, this will be "normal" memory, the default.
    #[must_use]
    #[doc(alias("cache", "cacheable", "non-cacheable"))]
    pub fn device_memory(mut self, enable: bool) -> Self {
        self.set(Self::DEVICE_MEMORY, enable);
        self
    }

    /// Returns a copy of this `PteFlags` with the `EXCLUSIVE` bit set or cleared.
    ///
    /// * If `enable` is `true`, this page will exclusively map its frame.
    /// * If `enable` is `false`, this page will NOT exclusively map its frame.
    #[must_use]
    pub fn exclusive(mut self, enable: bool) -> Self {
        self.set(Self::EXCLUSIVE, enable);
        self
    }

    #[doc(alias("present"))]
    pub fn is_valid(&self) -> bool {
        self.contains(Self::VALID)
    }

    #[doc(alias("read_only"))]
    pub fn is_writable(&self) -> bool {
        self.contains(Self::WRITABLE)
    }

    #[doc(alias("no_exec"))]
    pub fn is_executable(&self) -> bool {
        !self.contains(Self::NOT_EXECUTABLE)
    }

    #[doc(alias("cache", "cacheable", "non-cacheable"))]
    pub fn is_device_memory(&self) -> bool {
        self.contains(Self::DEVICE_MEMORY)
    }

    pub fn is_dirty(&self) -> bool {
        self.contains(Self::DIRTY)
    }

    pub fn is_accessed(&self) -> bool {
        self.contains(Self::ACCESSED)
    }

    pub fn is_exclusive(&self) -> bool {
        self.contains(Self::EXCLUSIVE)
    }
}
