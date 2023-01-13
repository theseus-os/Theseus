#![no_std]

pub type Mutex<T> = sync::Mutex<DisablePreemption, T>;
pub type MutexGuard<'a, T> = sync::MutexGuard<'a, DisablePreemption, T>;

pub struct DisablePreemption {}

impl sync::DeadlockPrevention for DisablePreemption {
    type GuardMarker = sync::GuardNoSend;

    #[inline]
    fn enter() {
        preemption::enable_preemption()
    }

    #[inline]
    fn exit() {
        preemption::disable_preemption()
    }
}
