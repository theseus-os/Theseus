//! Provides CPU-local storage (per-CPU variables) and preemption management.
//!
//! CPU-local variables cannot be used until after a given CPU has been initialized,
//! i.e., its Local APIC (on x86_64) has been discovered and properly configured.
//! Currently, the [`init()`] routine in this crate should be invoked by
//! another init routine from the `per_cpu` crate.
//!
//! Note that Rust offers the `#[thread_local]` attribute for thread-local storage (TLS),
//! but there is no equivalent for CPU-local storage.
//! On x86_64, TLS areas use the `fs` segment register for the TLS base,
//! and this crate uses the `gs` segment register for the CPU-local base.

#![no_std]
#![feature(negative_impls)]

extern crate alloc;

mod cpu_local;
mod preemption;

pub use cpu_local::*;
pub use preemption::*;
