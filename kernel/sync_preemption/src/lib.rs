#![no_std]

use preemption::{hold_preemption, PreemptionGuard};

pub type Mutex<T> = sync::Mutex<T, DisablePreemption>;
pub type MutexGuard<'a, T> = sync::MutexGuard<'a, T, DisablePreemption>;

/// A deadlock prevention method that disables preemption.
#[derive(Copy, Clone, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DisablePreemption {}

impl sync::DeadlockPrevention for DisablePreemption {
    type Guard = PreemptionGuard;

    const EXPENSIVE: bool = true;

    #[inline]
    fn enter() -> Self::Guard {
        hold_preemption()
    }
}
