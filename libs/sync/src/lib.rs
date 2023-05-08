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
    const MUTEX_INIT: Self::MutexData;

    const RW_LOCK_INIT: Self::RwLockData;

    type MutexData;

    type RwLockData;

    /// Additional guard stored in the synchronisation guards.
    type Guard;

    /// Tries to acquire the given mutex.
    fn try_lock_mutex<'a, T>(
        mutex: &'a spin::Mutex<T>,
        data: &'a Self::MutexData,
    ) -> Option<(spin::MutexGuard<'a, T>, Self::Guard)>
    where
        Self: Sized;

    /// Acquires the given mutex.
    fn lock_mutex<'a, T>(
        mutex: &'a spin::Mutex<T>,
        data: &'a Self::MutexData,
    ) -> (spin::MutexGuard<'a, T>, Self::Guard)
    where
        Self: Sized;

    /// Performs any necessary actions after unlocking the mutex.
    fn post_mutex_unlock(data: &Self::MutexData)
    where
        Self: Sized;

    fn try_read_rw_lock<'a, T>(
        rw_lock: &'a spin::RwLock<T>,
        data: &'a Self::RwLockData,
    ) -> Option<(spin::RwLockReadGuard<'a, T>, Self::Guard)>;

    fn try_write_rw_lock<'a, T>(
        rw_lock: &'a spin::RwLock<T>,
        data: &'a Self::RwLockData,
    ) -> Option<(spin::RwLockWriteGuard<'a, T>, Self::Guard)>;

    fn read_rw_lock<'a, T>(
        rw_lock: &'a spin::RwLock<T>,
        data: &'a Self::RwLockData,
    ) -> (spin::RwLockReadGuard<'a, T>, Self::Guard);

    fn write_rw_lock<'a, T>(
        rw_lock: &'a spin::RwLock<T>,
        data: &'a Self::RwLockData,
    ) -> (spin::RwLockWriteGuard<'a, T>, Self::Guard);

    fn post_rw_lock_unlock(data: &Self::RwLockData, is_writer_or_last_reader: bool);
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
    const MUTEX_INIT: Self::MutexData = ();

    const RW_LOCK_INIT: Self::RwLockData = ();

    type MutexData = ();

    type RwLockData = ();

    type Guard = <Self as DeadlockPrevention>::Guard;

    #[inline]
    fn try_lock_mutex<'a, T>(
        mutex: &'a spin::Mutex<T>,
        _: &'a Self::MutexData,
    ) -> Option<(spin::MutexGuard<'a, T>, Self::Guard)> {
        if Self::EXPENSIVE && mutex.is_locked() {
            return None;
        }

        let deadlock_guard = Self::enter();
        mutex.try_lock_weak().map(|guard| (guard, deadlock_guard))
    }

    #[inline]
    fn lock_mutex<'a, T>(
        mutex: &'a spin::Mutex<T>,
        _: &'a Self::MutexData,
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
    fn post_mutex_unlock(_: &Self::MutexData) {}

    #[inline]
    fn try_read_rw_lock<'a, T>(
        rw_lock: &'a spin::RwLock<T>,
        _: &'a Self::RwLockData,
    ) -> Option<(spin::RwLockReadGuard<'a, T>, Self::Guard)> {
        if Self::EXPENSIVE && rw_lock.writer_count() != 0 {
            return None;
        }

        let deadlock_guard = Self::enter();
        rw_lock.try_read().map(|guard| (guard, deadlock_guard))
    }

    #[inline]
    fn try_write_rw_lock<'a, T>(
        rw_lock: &'a spin::RwLock<T>,
        _: &'a Self::RwLockData,
    ) -> Option<(spin::RwLockWriteGuard<'a, T>, Self::Guard)> {
        if Self::EXPENSIVE && (rw_lock.reader_count() != 0 || rw_lock.writer_count() != 0){
            return None;
        }

        let deadlock_guard = Self::enter();
        rw_lock.try_write_weak().map(|guard| (guard, deadlock_guard))
    }

    #[inline]
    fn read_rw_lock<'a, T>(
        rw_lock: &'a spin::RwLock<T>,
        _: &'a Self::RwLockData,
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
    fn write_rw_lock<'a, T>(
        rw_lock: &'a spin::RwLock<T>,
        _: &'a Self::RwLockData,
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
    fn post_rw_lock_unlock(_: &Self::RwLockData, _: bool) {}
}
