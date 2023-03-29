#![feature(negative_impls)]
#![no_std]

use sync::{mutex, Flavour};
use sync_spin::Spin;
use wait_queue::WaitQueue;

/// A synchronisation flavour that blocks the current thread while waiting for
/// the lock to become available.
#[derive(Copy, Clone, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Block {}

impl !Send for Block {}

impl Flavour for Block {
    #[allow(clippy::declare_interior_mutable_const)]
    const INIT: Self::LockData = WaitQueue::new();

    type LockData = WaitQueue<Spin>;

    type DeadlockPrevention = Spin;

    #[inline]
    fn mutex_lock<'a, T>(
        mutex: &'a mutex::SpinMutex<Self::DeadlockPrevention, T>,
        data: &'a Self::LockData,
    ) -> mutex::SpinMutexGuard<'a, Self::DeadlockPrevention, T> {
        // TODO: try_lock_weak.
        if let Some(guard) = mutex.try_lock() {
            guard
        } else {
            data.wait_until(|| {
                // TODO: try_lock_weak
                mutex.try_lock()
            })
        }
    }

    #[inline]
    fn post_unlock(data: &Self::LockData) {
        data.notify_one();
    }
}
