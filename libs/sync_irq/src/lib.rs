#![no_std]

pub use irq_safety::{hold_interrupts, HeldInterrupts};

pub type Mutex<T> = sync::Mutex<T, DisableIrq>;
pub type MutexGuard<'a, T> = sync::MutexGuard<'a, T, DisableIrq>;

pub type IrqSafeMutex<T> = Mutex<T>;
pub type IrqSafeMutexGuard<'a, T> = MutexGuard<'a, T>;

pub type RwLock<T> = sync::RwLock<T, DisableIrq>;
pub type RwLockReadGuard<'a, T> = sync::RwLockReadGuard<'a, T, DisableIrq>;
pub type RwLockWriteGuard<'a, T> = sync::RwLockWriteGuard<'a, T, DisableIrq>;

pub type IrqSafeRwLock<T> = RwLock<T>;
pub type IrqSafeRwLockReadGuard<'a, T> = RwLockReadGuard<'a, T>;
pub type IrqSafeRwLockWriteGuard<'a, T> = RwLockWriteGuard<'a, T>;

/// A deadlock prevention method that disables interrupt requests.
#[derive(Copy, Clone, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DisableIrq {}

impl sync::DeadlockPrevention for DisableIrq {
    type Guard = HeldInterrupts;

    const EXPENSIVE: bool = true;

    #[inline]
    fn enter() -> Self::Guard {
        hold_interrupts("DeadlockPrevention::enter")
    }
}
