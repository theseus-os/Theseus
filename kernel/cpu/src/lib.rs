//! An abstraction for querying about CPUs (cores) in an SMP multicore system.
//!
//! This crate contains no extra functionality.
//! Currently it just re-exports types and functions from:
//! * [`apic`] on x86_64

#![no_std]

#[cfg_attr(target_arch = "x86_64", path = "x86_64.rs")]
#[cfg_attr(target_arch = "aarch64", path = "aarch64.rs")]
mod arch;

pub use arch::*;
