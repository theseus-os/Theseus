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

#[cfg_attr(target_arch = "x86_64", path = "x86_64.rs")]
#[cfg_attr(target_arch = "aarch64", path = "aarch64.rs")]
mod arch;

pub use arch::*;
