//! Allows configuring the Generic Interrupt Controller
//!
//! The term "Forwarding" is sometimes used in this crate.
//! This is because the Distributor, Redistributor and CPU interface are
//! chained in the controller. The distributor and the redistributor are
//! configured by the code of this crate to either allow (forward)
//! interrupts or disallow (discard) them.

#![no_std]
#![feature(doc_cfg)]
#![feature(array_try_from_fn)]

#[cfg(any(target_arch = "aarch64", doc))]
mod gic;

#[cfg(any(target_arch = "aarch64", doc))]
pub use gic::*;
