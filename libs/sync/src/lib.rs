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

pub use mutex::{Mutex, MutexFlavor, MutexGuard};
pub use rw_lock::{RwLock, RwLockFlavor, RwLockReadGuard, RwLockWriteGuard};

pub mod spin {
    pub use spin_rs::{
        mutex::spin::{SpinMutex as Mutex, SpinMutexGuard as MutexGuard},
        rwlock::{RwLock, RwLockReadGuard, RwLockWriteGuard},
    };
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

impl<P> MutexFlavor for P
where
    P: DeadlockPrevention,
{
    const INIT: Self::LockData = ();

    type LockData = ();

    type Guard = <Self as DeadlockPrevention>::Guard;

    #[inline]
    fn try_lock<'a, T>(
        mutex: &'a spin::Mutex<T>,
        _: &'a Self::LockData,
    ) -> Option<(spin::MutexGuard<'a, T>, Self::Guard)> {
        if Self::EXPENSIVE && mutex.is_locked() {
            return None;
        }

        let deadlock_guard = Self::enter();
        mutex.try_lock().map(|guard| (guard, deadlock_guard))
    }

    #[inline]
    fn lock<'a, T>(
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
    fn post_unlock(_: &Self::LockData) {}
}

impl<P> RwLockFlavor for P
where
    P: DeadlockPrevention,
{
    const INIT: Self::LockData = ();

    type LockData = ();

    type Guard = <Self as DeadlockPrevention>::Guard;

    #[inline]
    fn try_read<'a, T>(
        rw_lock: &'a spin::RwLock<T>,
        _: &'a Self::LockData,
    ) -> Option<(spin::RwLockReadGuard<'a, T>, Self::Guard)> {
        if Self::EXPENSIVE && rw_lock.writer_count() != 0 {
            return None;
        }

        let deadlock_guard = Self::enter();
        rw_lock.try_read().map(|guard| (guard, deadlock_guard))
    }

    #[inline]
    fn try_write<'a, T>(
        rw_lock: &'a spin::RwLock<T>,
        _: &'a Self::LockData,
    ) -> Option<(spin::RwLockWriteGuard<'a, T>, Self::Guard)> {
        if Self::EXPENSIVE && (rw_lock.reader_count() != 0 || rw_lock.writer_count() != 0) {
            return None;
        }

        let deadlock_guard = Self::enter();
        rw_lock.try_write().map(|guard| (guard, deadlock_guard))
    }

    #[inline]
    fn read<'a, T>(
        rw_lock: &'a spin::RwLock<T>,
        _: &'a Self::LockData,
    ) -> (spin::RwLockReadGuard<'a, T>, Self::Guard) {
        loop {
            let deadlock_guard = Self::enter();
            if let Some(guard) = rw_lock.try_read() {
                return (guard, deadlock_guard);
            }
            drop(deadlock_guard);

            while rw_lock.writer_count() != 0 {
                core::hint::spin_loop();
            }
        }
    }

    #[inline]
    fn write<'a, T>(
        rw_lock: &'a spin::RwLock<T>,
        _: &'a Self::LockData,
    ) -> (spin::RwLockWriteGuard<'a, T>, Self::Guard) {
        loop {
            let deadlock_guard = Self::enter();
            if let Some(guard) = rw_lock.try_write_weak() {
                return (guard, deadlock_guard);
            }
            drop(deadlock_guard);

            while rw_lock.writer_count() != 0 && rw_lock.reader_count() != 0 {
                core::hint::spin_loop();
            }
        }
    }

    #[inline]
    fn post_unlock(_: &Self::LockData, _: bool) {}
}
