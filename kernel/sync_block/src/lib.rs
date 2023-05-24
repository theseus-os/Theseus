#![feature(negative_impls)]
#![no_std]

use sync::{spin, MutexFlavor, RwLockFlavor};
use wait_queue::WaitQueue;

pub type Mutex<T> = sync::Mutex<T, Block>;
pub type MutexGuard<'a, T> = sync::MutexGuard<'a, T, Block>;

pub type RwLock<T> = sync::RwLock<T, Block>;
pub type RwLockReadGuard<'a, T> = sync::RwLockReadGuard<'a, T, Block>;
pub type RwLockWriteGuard<'a, T> = sync::RwLockWriteGuard<'a, T, Block>;

/// A synchronisation flavour that blocks the current thread while waiting for
/// the lock to become available.
#[derive(Copy, Clone, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Block {}

impl MutexFlavor for Block {
    #[allow(clippy::declare_interior_mutable_const)]
    const INIT: Self::LockData = WaitQueue::new();

    type LockData = WaitQueue;

    type Guard = ();

    #[inline]
    fn try_lock<'a, T>(
        mutex: &'a spin::Mutex<T>,
        _: &'a Self::LockData,
    ) -> Option<(spin::MutexGuard<'a, T>, Self::Guard)> {
        mutex.try_lock().map(|guard| (guard, ()))
    }

    #[inline]
    fn lock<'a, T>(
        mutex: &'a spin::Mutex<T>,
        data: &'a Self::LockData,
    ) -> (spin::MutexGuard<'a, T>, Self::Guard) {
        // This must be a strong compare exchange, otherwise we could block ourselves
        // when the mutex is unlocked and never be unblocked.
        if let Some(guards) = Self::try_lock(mutex, data) {
            guards
        } else {
            data.wait_until(|| Self::try_lock(mutex, data))
        }
    }

    #[inline]
    fn post_unlock(data: &Self::LockData) {
        data.notify_one();
    }
}

impl RwLockFlavor for Block {
    #[allow(clippy::declare_interior_mutable_const)]
    const INIT: Self::LockData = RwLockData {
        readers: WaitQueue::new(),
        writers: WaitQueue::new(),
    };

    type LockData = RwLockData;

    type Guard = ();

    #[inline]
    fn try_read<'a, T>(
        rw_lock: &'a spin::RwLock<T>,
        _: &'a Self::LockData,
    ) -> Option<(spin::RwLockReadGuard<'a, T>, Self::Guard)> {
        rw_lock.try_read().map(|guard| (guard, ()))
    }

    #[inline]
    fn try_write<'a, T>(
        rw_lock: &'a spin::RwLock<T>,
        _: &'a Self::LockData,
    ) -> Option<(spin::RwLockWriteGuard<'a, T>, Self::Guard)> {
        rw_lock.try_write().map(|guard| (guard, ()))
    }

    #[inline]
    fn read<'a, T>(
        rw_lock: &'a spin::RwLock<T>,
        data: &'a Self::LockData,
    ) -> (spin::RwLockReadGuard<'a, T>, Self::Guard) {
        if let Some(guards) = Self::try_read(rw_lock, data) {
            guards
        } else {
            data.readers.wait_until(|| Self::try_read(rw_lock, data))
        }
    }

    #[inline]
    fn write<'a, T>(
        rw_lock: &'a spin::RwLock<T>,
        data: &'a Self::LockData,
    ) -> (spin::RwLockWriteGuard<'a, T>, Self::Guard) {
        if let Some(guards) = Self::try_write(rw_lock, data) {
            guards
        } else {
            data.writers.wait_until(|| Self::try_write(rw_lock, data))
        }
    }

    #[inline]
    fn post_unlock(data: &Self::LockData, is_writer_or_last_reader: bool) {
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
