#![feature(negative_impls)]
#![no_std]

pub type Mutex<T> = sync::Mutex<DisableIrq, T>;
pub type MutexGuard<'a, T> = sync::MutexGuard<'a, DisableIrq, T>;

/// A deadlock prevention method that disables interrupt requests.
#[derive(Copy, Clone, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DisableIrq {}

impl sync::DeadlockPrevention for DisableIrq {
    type Guard = HeldInterrupts;

    const EXPENSIVE: bool = true;

    #[inline]
    fn enter() -> Self::Guard {
        irq_safety::hold_interrupts();
    }
}
