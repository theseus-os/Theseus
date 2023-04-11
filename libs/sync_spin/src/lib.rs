#![no_std]

pub type Mutex<T> = sync::Mutex<T, Spin>;
pub type MutexGuard<'a, T> = sync::MutexGuard<'a, T, Spin>;

/// A no-op deadlock prevention method.
#[derive(Copy, Clone, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Spin {}

impl sync::DeadlockPrevention for Spin {
    type Guard = ();

    const EXPENSIVE: bool = false;

    #[inline]
    fn enter() -> Self::Guard {
        ()
    }
}
