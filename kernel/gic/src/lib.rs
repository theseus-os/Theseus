//! Arm Generic Interrupt Controller Support
//!
//! The GIC is an extension to ARMv8 which allows routing and
//! filtering interrupts in a single or multi-core system.
//!
//! The term "Forwarding" is sometimes used in this crate.
//! This is because the Distributor, Redistributor and CPU interface are
//! chained in the controller. The distributor and the redistributor are
//! configured by the code of this crate to either allow (forward)
//! interrupts or disallow (discard) them.

#![no_std]
#![feature(doc_cfg)]

#[cfg(target_arch = "aarch64")]
mod gic;

#[cfg(target_arch = "aarch64")]
pub use gic::*;
