//! Support for the Page Attribute Table (PAT) feature on x86.
//!
//! PAT replaces the legacy Memory Type Range Registers (MTRRs) on x86.
//! PAT allows the system to assign memory types to regions of "linear" (virtual) addresses
//! instead of the MTRRs, which operate on regions of physical addresses.
//! 

#![no_std]

use log::*;

use raw_cpuid::CpuId;

use x86_64::registers::control::{Cr0, Cr0Flags};




pub const PAT_MSR: u32 = msr::IA32_PAT;

#[doc(alias("pat", "mtrr", "page attribute table"))]
pub enum MemoryCachingType {
    Uncacheable = 0x00,
    WriteCombining = 0x01,
    // 0x02 and 0x03 are reserved
    WriteThrough = 0x04,
    WriteProtected = 0x05,
    WriteBack = 0x06,
    Uncached = 0x07,
    // All other values are reserved.
}

pub struct PatMsr {
}