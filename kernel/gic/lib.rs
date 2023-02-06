//! Allows configuring the Generic Interrupt Controller
#![no_std]
#![feature(doc_cfg)]

#[cfg(any(target_arch = "aarch64", doc))]
mod src;

#[cfg(any(target_arch = "aarch64", doc))]
pub use src::*;
