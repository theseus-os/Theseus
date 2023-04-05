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

pub use mutex::{Mutex, MutexGuard};

/// A synchronisation flavour.
pub trait Flavour {
    /// Initial value for the lock data.
    const INIT: Self::LockData;

    /// Additional data stored in the lock.
    type LockData;

    type Guard;

    /// Tries to acquire the given mutex.
    fn mutex_try_lock<'a, T>(
        mutex: &'a mutex::SpinMutex<T>,
        data: &'a Self::LockData,
    ) -> Option<(mutex::SpinMutexGuard<'a, T>, Self::Guard)>
    where
        Self: Sized;

    /// Acquires the given mutex.
    fn mutex_lock<'a, T>(
        mutex: &'a mutex::SpinMutex<T>,
        data: &'a Self::LockData,
    ) -> (mutex::SpinMutexGuard<'a, T>, Self::Guard)
    where
        Self: Sized;

    /// Performs any necessary actions after unlocking the mutex.
    ///
    /// This runs after the deadlock prevention guard has been dropped.
    fn post_unlock(mutex: &Self::LockData)
    where
        Self: Sized;
}

/// A deadlock prevention method.
pub trait DeadlockPrevention {
    /// A guard that is stored in the mutex guard.
    type Guard;

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
    fn mutex_try_lock<'a, T>(
        mutex: &'a mutex::SpinMutex<T>,
        _: &'a Self::LockData,
    ) -> Option<(mutex::SpinMutexGuard<'a, T>, Self::Guard)> {
        let deadlock_guard = Self::enter();

        if let Some(guard) = mutex.try_lock() {
            Some((guard, deadlock_guard))
        } else {
            None
        }
    }

    #[inline]
    fn mutex_lock<'a, T>(
        mutex: &'a mutex::SpinMutex<T>,
        _: &'a Self::LockData,
    ) -> (mutex::SpinMutexGuard<'a, T>, Self::Guard) {
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
