#![feature(negative_impls)]
#![no_std]

use preemption::{hold_preemption, PreemptionGuard};

pub type Mutex<T> = sync::Mutex<DisablePreemption, T>;
pub type MutexGuard<'a, T> = sync::MutexGuard<'a, DisablePreemption, T>;

/// A deadlock prevention method that disables preemption.
#[derive(Copy, Clone, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DisablePreemption {}

#[doc(hidden)]
pub struct Guard(PreemptionGuard);

impl !Send for Guard {}

impl sync::DeadlockPrevention for DisablePreemption {
    type Guard = Guard;

    #[inline]
    fn enter() -> Self::Guard {
        Guard(hold_preemption())
    }
}
