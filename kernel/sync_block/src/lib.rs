#![feature(negative_impls)]
#![no_std]

use sync::{mutex, Flavour};
use sync_spin::Spin;
use wait_queue::WaitQueue;

/// A synchronisation flavour that blocks the current thread while waiting for
/// the lock to become available.
#[derive(Copy, Clone, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Block {}

impl Flavour for Block {
    #[allow(clippy::declare_interior_mutable_const)]
    const INIT: Self::LockData = WaitQueue::new();

    type LockData = WaitQueue<Spin>;

    type Guard = ();

    #[inline]
    fn try_lock_mutex<'a, T>(
        mutex: &'a mutex::SpinMutex<T>,
        _: &'a Self::LockData,
    ) -> Option<(mutex::SpinMutexGuard<'a, T>, Self::Guard)> {
        mutex.try_lock_weak().map(|guard| (guard, ()))
    }

    #[inline]
    fn lock_mutex<'a, T>(
        mutex: &'a mutex::SpinMutex<T>,
        data: &'a Self::LockData,
    ) -> (mutex::SpinMutexGuard<'a, T>, Self::Guard) {
        if let Some(guard) = mutex.try_lock_weak() {
            (guard, ())
        } else {
            (data.wait_until(|| mutex.try_lock_weak()), ())
        }
    }

    #[inline]
    fn post_unlock(data: &Self::LockData) {
        data.notify_one();
    }
}
