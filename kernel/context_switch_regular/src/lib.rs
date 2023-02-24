#![no_std]
#![feature(naked_functions)]

extern crate zerocopy;

#[cfg(target_arch = "x86_64")]
#[path = "x86_64.rs"]
mod arch;

#[cfg(target_arch = "aarch64")]
#[path = "aarch64.rs"]
mod arch;

pub use arch::*;