//! An abstraction for querying about CPUs (cores) in an SMP multicore system.
//!
//! This crate contains no extra functionality.
//! Currently it just re-exports types and functions from:
//! * [`apic`] on x86_64

#![no_std]

#[cfg(target_arch = "x86_64")]
pub use apic::{
    CpuId,
    cpu_count,
    bootstrap_cpu,
    is_bootstrap_cpu,
    current_cpu,
};
