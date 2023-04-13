//! Only synchronisation primitive implementations should depend on this crate.
//!
//! If a crate uses a synchronisation primitive, it should depend on one of the
//! following:
//! - `sync_spin`
//! - `sync_preemption`
//! - `sync_irq`
//! - `sync_block`

#![no_std]

pub mod mutex;
pub mod rw_lock;

pub use mutex::{Mutex, MutexGuard};
pub use rw_lock::{RwLock, RwLockReadGuard, RwLockWriteGuard};

pub mod spin {
    pub use spin_rs::{
        mutex::spin::{SpinMutex as Mutex, SpinMutexGuard as MutexGuard},
        rwlock::{RwLock, RwLockReadGuard, RwLockWriteGuard},
    };
}

/// A synchronisation flavour.
pub trait Flavour {
    const INIT: Self::LockData;

    /// Additional data stored in the lock.
    type LockData;

    /// Additional guard stored in the synchronisation guards.
    type Guard;

    /// Tries to acquire the given mutex.
    fn try_lock_mutex<'a, T>(
        mutex: &'a spin::Mutex<T>,
        data: &'a Self::LockData,
    ) -> Option<(spin::MutexGuard<'a, T>, Self::Guard)>
    where
        Self: Sized;

    /// Acquires the given mutex.
    fn lock_mutex<'a, T>(
        mutex: &'a spin::Mutex<T>,
        data: &'a Self::LockData,
    ) -> (spin::MutexGuard<'a, T>, Self::Guard)
    where
        Self: Sized;

    /// Performs any necessary actions after unlocking the mutex.
    fn post_mutex_unlock(data: &Self::LockData)
    where
        Self: Sized;

    fn try_read_rw_lock<'a, T>(
        rw_lock: &'a spin::RwLock<T>,
        data: &'a Self::LockData,
    ) -> Option<(spin::RwLockReadGuard<'a, T>, Self::Guard)>;

    fn try_write_rw_lock<'a, T>(
        rw_lock: &'a spin::RwLock<T>,
        data: &'a Self::LockData,
    ) -> Option<(spin::RwLockWriteGuard<'a, T>, Self::Guard)>;

    fn read_rw_lock<'a, T>(
        rw_lock: &'a spin::RwLock<T>,
        data: &'a Self::LockData,
    ) -> (spin::RwLockReadGuard<'a, T>, Self::Guard);

    fn write_rw_lock<'a, T>(
        rw_lock: &'a spin::RwLock<T>,
        data: &'a Self::LockData,
    ) -> (spin::RwLockWriteGuard<'a, T>, Self::Guard);

    fn post_rw_lock_unlock(data: &Self::LockData, is_last_reader: bool);
}

/// A deadlock prevention method.
pub trait DeadlockPrevention {
    /// Additional guard stored in the synchronisation guards.
    type Guard;

    /// Whether entering the deadlock prevention context is *expensive*.
    ///
    /// This determines whether to check that the mutex is locked before
    /// attempting to lock the mutex in `try_lock`.
    const EXPENSIVE: bool;

    /// Enters the deadlock prevention context.
    fn enter() -> Self::Guard;
}

impl<P> Flavour for P
where
    P: DeadlockPrevention,
{
    const INIT: Self::LockData = ();

    type LockData = ();

    type Guard = <Self as DeadlockPrevention>::Guard;

    #[inline]
    fn try_lock_mutex<'a, T>(
        mutex: &'a spin::Mutex<T>,
        _: &'a Self::LockData,
    ) -> Option<(spin::MutexGuard<'a, T>, Self::Guard)> {
        // FIXME: Use `is_locked_acquire`.
        if Self::EXPENSIVE && mutex.is_locked() {
            return None;
        }

        let deadlock_guard = Self::enter();
        mutex.try_lock().map(|guard| (guard, deadlock_guard))
    }

    #[inline]
    fn lock_mutex<'a, T>(
        mutex: &'a spin::Mutex<T>,
        _: &'a Self::LockData,
    ) -> (spin::MutexGuard<'a, T>, Self::Guard) {
        loop {
            let deadlock_guard = Self::enter();
            if let Some(guard) = mutex.try_lock_weak() {
                return (guard, deadlock_guard);
            }
            drop(deadlock_guard);

            while mutex.is_locked() {
                core::hint::spin_loop();
            }
        }
    }

    #[inline]
    fn post_mutex_unlock(_: &Self::LockData) {}

    #[inline]
    fn try_read_rw_lock<'a, T>(
        rw_lock: &'a spin::RwLock<T>,
        _: &'a Self::LockData,
    ) -> Option<(spin::RwLockReadGuard<'a, T>, Self::Guard)> {
        // TODO: Fastpath?

        let deadlock_guard = Self::enter();
        rw_lock.try_read().map(|guard| (guard, deadlock_guard))
    }

    #[inline]
    fn try_write_rw_lock<'a, T>(
        rw_lock: &'a spin::RwLock<T>,
        _: &'a Self::LockData,
    ) -> Option<(spin::RwLockWriteGuard<'a, T>, Self::Guard)> {
        // TODO: Fastpath?

        let deadlock_guard = Self::enter();
        rw_lock.try_write().map(|guard| (guard, deadlock_guard))
    }

    #[inline]
    fn read_rw_lock<'a, T>(
        _rw_lock: &'a spin::RwLock<T>,
        _: &'a Self::LockData,
    ) -> (spin::RwLockReadGuard<'a, T>, Self::Guard) {
        todo!();
    }

    #[inline]
    fn write_rw_lock<'a, T>(
        _rw_lock: &'a spin::RwLock<T>,
        _: &'a Self::LockData,
    ) -> (spin::RwLockWriteGuard<'a, T>, Self::Guard) {
        todo!();
    }

    #[inline]
    fn post_rw_lock_unlock(_: &Self::LockData, _: bool) {}
}
