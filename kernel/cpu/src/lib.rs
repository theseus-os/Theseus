//! An abstraction for querying about CPUs (cores) in an SMP multicore system.
//!
//! This crate contains no extra functionality.
//! Currently it consists of:
//! * re-exports of items from [`apic`] on x86_64
//! * canonical definitions on aarch64
//!
//! Note: This crate currently assumes there is only one available CPU core in
//! the system on Arm, as secondary cores are currently unused in Theseus on Arm.

#![no_std]

#[cfg(target_arch = "aarch64")]
extern crate alloc;

#[cfg_attr(target_arch = "x86_64", path = "x86_64.rs")]
#[cfg_attr(target_arch = "aarch64", path = "aarch64.rs")]
mod arch;

pub use arch::*;

use derive_more::*;

/// A unique identifier for a CPU core.
///
/// A `CpuId` is a known-valid value that is guaranteed to correspond
/// to a single CPU that actually exists on the current system.
#[derive(
    Clone, Copy, Debug, Display, PartialEq, Eq, PartialOrd, Ord,
    Hash, Binary, Octal, LowerHex, UpperHex,
)]
#[repr(transparent)]
pub struct CpuId(u32);

impl CpuId {
    /// Returns the inner raw value of this `CpuId`.
    pub fn value(&self) -> u32 {
        self.0
    }

    /// Returns `true` if this `CpuId` is the ID of the bootstrap CPU,
    /// the first CPU to boot.
    pub fn is_bootstrap_cpu(&self) -> bool {
        Some(self) == arch::bootstrap_cpu().as_ref()
    }

    /// A temporary function (will be removed later) that converts the given `CpuId`
    /// into a `u8`, panicking if its inner `u32` value does not fit into a `u8`.
    pub fn into_u8(self) -> u8 {
        self.0
            .try_into()
            .unwrap_or_else(|_| panic!("couldn't convert CpuId {self} into a u8"))
    }
}

impl From<CpuId> for u32 {
    fn from(value: CpuId) -> Self {
        value.0
    }
}
impl From<CpuId> for u64 {
    fn from(value: CpuId) -> Self {
        value.0.into()
    }
}
