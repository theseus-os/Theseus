//! Support for the Page Attribute Table (PAT) feature on x86.
//!
//! PAT replaces the legacy Memory Type Range Registers (MTRRs) on x86.
//! PAT allows the system to assign memory types, i.e., specify their caching behavior,
//! to regions of "linear" (virtual) addresses.
//! This is in contrast to MTRRs, which operate on regions of physical addresses.

#![no_std]

use log::*;
use modular_bitfield::{specifiers::{B3, B5}, bitfield};
use msr::IA32_PAT;
use raw_cpuid::CpuId;
use spin::Once;
// use x86_64::registers::control::{Cr0, Cr0Flags};

/// Theseus's fixed [`PageAttributeTable`] has slots that align with
/// the default meaning of page table entry bits on x86_64.
///
/// | Bit Position | If Clear (0)  | If Set (1)     |
/// |:-------------|:--------------|:---------------|
/// | 0  (LSB)     | write back    | write through  |
/// | 1  (middle)  | cache enabled | cache disabled |
/// | 2  (MSB)     | N/A           | N/A            |
///
/// Thus, the following slots are chosen to align with those bits:
/// * Slot 0 (`0b000`): [MemoryCachingType::WriteBack]
/// * Slot 1 (`0b001`): [MemoryCachingType::WriteThrough]
/// * Slot 2 (`0b010`): [MemoryCachingType::Uncacheable] (caching disabled)
/// * Slot 3 (`0b011`): unused -- write through and cache enabled doesn't make sense,
///   but this is set up as `Uncacheable` because the cache disabled flag takes priority.
///
/// The following slots are available for custom use,
/// and Theseus currently sets them up as such:
/// * Slot 4 (`0b100`): [MemoryCachingType::WriteProtected]
/// * Slot 5 (`0b101`): [MemoryCachingType::WriteCombining]
/// * Slot 6 (`0b110`): [MemoryCachingType::UncachedMinus]
/// * Slot 7 (`0b111`): [MemoryCachingType::UncachedMinus]
///
/// Currently, the difference between `Uncacheable` and
/// `UncachedMinus` is not clear, so we offer slots for both.
///
/// ## Usage
/// You cannot and do not need to use this type directly, as it is
/// pre-set up statically for you.
/// Instead, use [`MemoryCachingType::pat_slot_index()`] to obtain
/// the index of the PAT slot that has been set up for whatever
/// `MemoryCachingType` you need. 
pub static FIXED_PAT: PageAttributeTable = PageAttributeTable::from_bytes([
    //
    // NOTE: this order must be kept in sync with `MemoryCachingType::pat_slot_index`.
    //
    MemoryCachingType::WriteBack      as u8, // 0: 0b000 
    MemoryCachingType::WriteThrough   as u8, // 1: 0b001
    MemoryCachingType::Uncacheable    as u8, // 2: 0b010
    MemoryCachingType::Uncacheable    as u8, // 3: 0b011
    MemoryCachingType::WriteProtected as u8, // 4: 0b100
    MemoryCachingType::WriteCombining as u8, // 5: 0b101
    MemoryCachingType::UncachedMinus  as u8, // 6: 0b110
    MemoryCachingType::UncachedMinus  as u8, // 7: 0b111
]);

/// The Page Attribute Table (PAT) consists of 8 "slots" that can each
/// be configured with a different [MemoryCachingType].
#[bitfield(bits = 64)]
#[repr(u64)]
#[derive(Copy, Clone)]
pub struct PageAttributeTable {
    #[skip] pat_slot_0: B3,
    #[skip] _reserved0: B5,
    #[skip] pat_slot_1: B3,
    #[skip] _reserved1: B5,
    #[skip] pat_slot_2: B3,
    #[skip] _reserved2: B5,
    #[skip] pat_slot_3: B3,
    #[skip] _reserved3: B5,
    #[skip] pat_slot_4: B3,
    #[skip] _reserved4: B5,
    #[skip] pat_slot_5: B3,
    #[skip] _reserved5: B5,
    #[skip] pat_slot_6: B3,
    #[skip] _reserved6: B5,
    #[skip] pat_slot_7: B3,
    #[skip] _reserved7: B5,
}


/// The various types of memory caching that x86 supports
/// for usage in the [`PageAttributeTable`].
///
/// The default is [`MemoryCachingType::WriteBack`],
/// which corresponds to the standard default caching mode
/// when no specific page table entry flags are set.
#[doc(alias("pat", "mtrr", "page attribute table"))]
#[derive(Debug, Copy, Clone)]
#[repr(u8)]
pub enum MemoryCachingType {
    Uncacheable    = 0x00,
    WriteCombining = 0x01,
    // 0x02 and 0x03 are reserved.
    WriteThrough   = 0x04,
    WriteProtected = 0x05,
    WriteBack      = 0x06,
    UncachedMinus  = 0x07,
    // All other values are reserved.
}
impl MemoryCachingType {
    /// Returns the index of the [`PageAttributeTable`] (PAT) slot
    /// that has been pre-configured with this `MemoryCachingType`.
    ///
    /// See the docs of [FIXED_PAT] for more info.
    pub const fn pat_slot_index(self) -> u8 {
        //
        // NOTE: this must be kept in sync with the definition of `FIXED_PAT`.
        //
        match self {
            Self::WriteBack      => 0,
            Self::WriteThrough   => 1,
            Self::Uncacheable    => 2, // also 3
            Self::WriteProtected => 4,
            Self::WriteCombining => 5,
            Self::UncachedMinus  => 6, // also 6
        }
    }
}

/// Returns `true` if the Page Attribute Table is supported on this system.
pub fn is_supported() -> bool {
    // Cache the result of CpuId 
    static PAT_SUPPORT: Once<bool> = Once::new();
    
    *PAT_SUPPORT.call_once(|| CpuId::new()
        .get_feature_info()
        .map(|finfo| finfo.has_pat())
        .unwrap_or(false)
    )
}

/// An empty error type indicating that the Page Attribute Table
/// is not supported on this machine.
#[derive(Debug)]
pub struct PatNotSupported;

/// Sets up and enables the Page Attribute Table (PAT) for this (the current) CPU.
///
/// This works by setting the [`IA32_PAT`] MSR to the value
/// specified by [`FIXED_PAT`];
/// thus, it must be done separately on each and every CPU.
///
/// Returns `Ok(())` upon success, and `Err(PatNotSupported::Unsupported)` if PAT is unsupported.
pub fn init() -> Result<(), PatNotSupported> {
    if !is_supported() {
        return Err(PatNotSupported);
    }

    // TODO: do we need to disable the cache using CR0 first? or flush it?

    let mut pat_msr = x86_64::registers::model_specific::Msr::new(IA32_PAT);
    unsafe {
        pat_msr.write(FIXED_PAT.into());
    }

    debug!("Enabled the Page Attribute Table for the current CPU.");
    Ok(())
}
