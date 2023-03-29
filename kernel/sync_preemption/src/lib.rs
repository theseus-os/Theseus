#![feature(negative_impls)]
#![no_std]

pub type Mutex<T> = sync::Mutex<DisablePreemption, T>;
pub type MutexGuard<'a, T> = sync::MutexGuard<'a, DisablePreemption, T>;

/// A deadlock prevention method that disables preemption.
#[derive(Copy, Clone, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DisablePreemption {}

impl !Send for DisablePreemption {}

impl sync::DeadlockPrevention for DisablePreemption {
    #[inline]
    fn enter() {
        preemption::disable_preemption()
    }

    #[inline]
    fn exit() {
        preemption::enable_preemption()
    }
}
