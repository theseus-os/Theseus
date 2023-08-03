//! A library for defining CPU-local variables.
//!
//! See [`cpu_local`] for more details.

#![no_std]

extern crate alloc;

pub use cls_macros::cpu_local;

pub trait Guard: sealed::Sealed {}

impl sealed::Sealed for irq_safety::HeldInterrupts {}
impl Guard for irq_safety::HeldInterrupts {}

impl sealed::Sealed for preemption::PreemptionGuard {}
impl Guard for preemption::PreemptionGuard {}

mod sealed {
    pub trait Sealed {}
}
