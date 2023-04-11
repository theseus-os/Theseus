#![feature(negative_impls)]
#![no_std]

use sync::{mutex, Flavour};
use wait_queue::WaitQueue;

/// A synchronisation flavour that blocks the current thread while waiting for
/// the lock to become available.
#[derive(Copy, Clone, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Block {}

impl Flavour for Block {
    #[allow(clippy::declare_interior_mutable_const)]
    const INIT: Self::LockData = WaitQueue::new();

    type LockData = WaitQueue;

    type Guard = ();

    #[inline]
    fn try_lock_mutex<'a, T>(
        mutex: &'a mutex::SpinMutex<T>,
        _: &'a Self::LockData,
    ) -> Option<(mutex::SpinMutexGuard<'a, T>, Self::Guard)> {
        mutex.try_lock().map(|guard| (guard, ()))
    }

    #[inline]
    fn lock_mutex<'a, T>(
        mutex: &'a mutex::SpinMutex<T>,
        data: &'a Self::LockData,
    ) -> (mutex::SpinMutexGuard<'a, T>, Self::Guard) {
        // This must be a strong compare exchange, otherwise we could block ourselves
        // when the mutex is unlocked and never be unblocked.
        if let Some(guards) = Self::try_lock_mutex(mutex, data) {
            guards
        } else {
            data.wait_until(|| Self::try_lock_mutex(mutex, data))
        }
    }

    #[inline]
    fn post_unlock(data: &Self::LockData) {
        data.notify_one();
    }
}
