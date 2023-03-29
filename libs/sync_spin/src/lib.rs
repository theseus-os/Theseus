#![no_std]

pub type Mutex<T> = sync::Mutex<Spin, T>;
pub type MutexGuard<'a, T> = sync::MutexGuard<'a, Spin, T>;

/// A no-op deadlock prevention method.
#[derive(Copy, Clone, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Spin {}

impl sync::DeadlockPrevention for Spin {
    #[inline]
    fn enter() {}

    #[inline]
    fn exit() {}
}
