#![no_std]

use irq_safety::{hold_interrupts, HeldInterrupts};

pub type Mutex<T> = sync::Mutex<T, DisableIrq>;
pub type MutexGuard<'a, T> = sync::MutexGuard<'a, T, DisableIrq>;

/// A deadlock prevention method that disables interrupt requests.
#[derive(Copy, Clone, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DisableIrq {}

impl sync::DeadlockPrevention for DisableIrq {
    type Guard = HeldInterrupts;

    const EXPENSIVE: bool = true;

    #[inline]
    fn enter() -> Self::Guard {
        hold_interrupts()
    }
}
