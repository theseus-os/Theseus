//! Offers preemption-safe `Mutex` and `RwLock` types that auto-disable/re-enabled preemption
//! on a per-CPU core basis.

#![no_std]
#![feature(negative_impls)]

extern crate alloc;

mod mutex_preempt;
mod rwlock_preempt;

pub use mutex_preempt::*;
pub use rwlock_preempt::*;
