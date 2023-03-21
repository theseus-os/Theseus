//! Initialization and bring-up of secondary CPUs.
//!
//! These functions are intended to be invoked from the BSP
//! (the Bootstrap Processor, the main CPU in x86 terms)
//! in order to bring up secondary CPUs (APs in x86 terms).

#![no_std]
#![cfg_attr(target_arch = "x86_64", feature(let_chains))]
#![cfg_attr(target_arch = "aarch64", feature(naked_functions))]

#[cfg_attr(target_arch = "x86_64", path = "x86_64.rs")]
#[cfg_attr(target_arch = "aarch64", path = "aarch64.rs")]
mod arch;

pub use arch::*;
