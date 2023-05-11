#![feature(negative_impls)]
#![no_std]

mod condvar;

use sync::{spin, Flavour};
use wait_queue::WaitQueue;

pub use condvar::Condvar;

pub type Mutex<T> = sync::Mutex<T, Block>;
pub type MutexGuard<'a, T> = sync::MutexGuard<'a, T, Block>;

pub type RwLock<T> = sync::RwLock<T, Block>;
pub type RwLockReadGuard<'a, T> = sync::RwLockReadGuard<'a, T, Block>;
pub type RwLockWriteGuard<'a, T> = sync::RwLockWriteGuard<'a, T, Block>;

/// A synchronisation flavour that blocks the current thread while waiting for
/// the lock to become available.
#[derive(Copy, Clone, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Block {}

impl Flavour for Block {
    #[allow(clippy::declare_interior_mutable_const)]
    const MUTEX_INIT: Self::MutexData = WaitQueue::new();

    #[allow(clippy::declare_interior_mutable_const)]
    const RW_LOCK_INIT: Self::RwLockData = RwLockData {
        readers: WaitQueue::new(),
        writers: WaitQueue::new(),
    };

    type MutexData = WaitQueue;

    type RwLockData = RwLockData;

    type Guard = ();

    #[inline]
    fn try_lock_mutex<'a, T>(
        mutex: &'a spin::Mutex<T>,
        _: &'a Self::MutexData,
    ) -> Option<(spin::MutexGuard<'a, T>, Self::Guard)> {
        mutex.try_lock().map(|guard| (guard, ()))
    }

    #[inline]
    fn lock_mutex<'a, T>(
        mutex: &'a spin::Mutex<T>,
        data: &'a Self::MutexData,
    ) -> (spin::MutexGuard<'a, T>, Self::Guard) {
        // This must be a strong compare exchange, otherwise we could block ourselves
        // when the mutex is unlocked and never be unblocked.
        if let Some(guards) = Self::try_lock_mutex(mutex, data) {
            guards
        } else {
            data.wait_until(|| Self::try_lock_mutex(mutex, data))
        }
    }

    #[inline]
    fn post_mutex_unlock(data: &Self::MutexData) {
        data.notify_one();
    }

    #[inline]
    fn try_read_rw_lock<'a, T>(
        rw_lock: &'a spin::RwLock<T>,
        _: &'a Self::RwLockData,
    ) -> Option<(spin::RwLockReadGuard<'a, T>, Self::Guard)> {
        rw_lock.try_read().map(|guard| (guard, ()))
    }

    #[inline]
    fn try_write_rw_lock<'a, T>(
        rw_lock: &'a spin::RwLock<T>,
        _: &'a Self::RwLockData,
    ) -> Option<(spin::RwLockWriteGuard<'a, T>, Self::Guard)> {
        rw_lock.try_write().map(|guard| (guard, ()))
    }

    #[inline]
    fn read_rw_lock<'a, T>(
        rw_lock: &'a spin::RwLock<T>,
        data: &'a Self::RwLockData,
    ) -> (spin::RwLockReadGuard<'a, T>, Self::Guard) {
        if let Some(guards) = Self::try_read_rw_lock(rw_lock, data) {
            guards
        } else {
            data.readers
                .wait_until(|| Self::try_read_rw_lock(rw_lock, data))
        }
    }

    #[inline]
    fn write_rw_lock<'a, T>(
        rw_lock: &'a spin::RwLock<T>,
        data: &'a Self::RwLockData,
    ) -> (spin::RwLockWriteGuard<'a, T>, Self::Guard) {
        // We must not use try_write_weak because that could lead to us waiting forever.
        if let Some(guards) = Self::try_write_rw_lock(rw_lock, data) {
            guards
        } else {
            data.writers
                .wait_until(|| Self::try_write_rw_lock(rw_lock, data))
        }
    }

    #[inline]
    fn post_rw_lock_unlock(data: &Self::RwLockData, is_writer_or_last_reader: bool) {
        if is_writer_or_last_reader && !data.writers.notify_one() {
            data.readers.notify_all();
        }
    }
}

#[doc(hidden)]
pub struct RwLockData {
    readers: WaitQueue,
    writers: WaitQueue,
}
