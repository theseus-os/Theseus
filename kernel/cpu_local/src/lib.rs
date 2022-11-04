//! Offers types and macros to declare and access CPU-local storage (per-CPU variables).
//!
//! CPU-local variables cannot be used until after a given CPU has bee initialized,
//! i.e., its Local APIC (on x86_64) has been discovered and properly configured.
//!
//! Note that Rust offers the `#[thread_local]` attribute for thread-local storage (TLS),
//! but there is no equivalent for CPU-local storage.
//! On x86_64, TLS areas use the `fs` segment register for the TLS base,
//! and this crates uses the `gs` segment register for the CPU-local base.

#![no_std]

extern crate alloc;

// TODO
