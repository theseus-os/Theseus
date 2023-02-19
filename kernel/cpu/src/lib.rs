//! An abstraction for querying about CPUs (cores) in an SMP multicore system.
//!
//! This crate contains no extra functionality.
//! Currently it just re-exports types and functions from:
//! * [`apic`] on x86_64

#![no_std]

mod arch;
pub use arch::*;
