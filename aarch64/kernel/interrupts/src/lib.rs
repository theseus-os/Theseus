//! Basic interrupt handling structures and simple handler routines.

#![no_std]

#[cfg(target_arch = "x86_64")]
#[path = "x86_64/mod.rs"]
mod arch;

#[cfg(target_arch = "aarch64")]
#[path = "aarch64/mod.rs"]
mod arch;

pub use arch::*;
