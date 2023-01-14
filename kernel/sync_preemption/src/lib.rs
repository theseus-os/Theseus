#![no_std]

pub type Mutex<T> = sync::Mutex<DisablePreemption, T>;
pub type MutexGuard<'a, T> = sync::MutexGuard<'a, DisablePreemption, T>;

/// A deadlock prevention method that disables preemption.
#[derive(Copy, Clone, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DisablePreemption {}

impl sync::DeadlockPrevention for DisablePreemption {
    type GuardMarker = sync::GuardNoSend;

    #[inline]
    fn enter() {
        preemption::disable_preemption()
    }

    #[inline]
    fn exit() {
        preemption::enable_preemption()
    }
}
