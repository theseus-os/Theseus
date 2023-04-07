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
    // /// Initial value for the lock data.
    // const INIT: Self::LockData;

    /// Additional data stored in the lock.
    type LockData;

    /// Additional guard stored in the synchronisation guards.
    type Guard;

    fn new() -> Self::LockData;

    /// Tries to acquire the given mutex.
    fn try_lock_mutex<'a, T>(
        mutex: &'a mutex::SpinMutex<T>,
        data: &'a Self::LockData,
    ) -> Option<(mutex::SpinMutexGuard<'a, T>, Self::Guard)>
    where
        Self: Sized;

    /// Acquires the given mutex.
    fn lock_mutex<'a, T>(
        mutex: &'a mutex::SpinMutex<T>,
        data: &'a Self::LockData,
    ) -> (mutex::SpinMutexGuard<'a, T>, Self::Guard)
    where
        Self: Sized;

    /// Performs any necessary actions after unlocking the mutex.
    fn post_unlock(mutex: &Self::LockData)
    where
        Self: Sized;
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
    // const INIT: Self::LockData = ();

    type LockData = ();

    type Guard = <Self as DeadlockPrevention>::Guard;

    fn new() -> Self::LockData {
        ()
    }

    #[inline]
    fn try_lock_mutex<'a, T>(
        mutex: &'a mutex::SpinMutex<T>,
        _: &'a Self::LockData,
    ) -> Option<(mutex::SpinMutexGuard<'a, T>, Self::Guard)> {
        if Self::EXPENSIVE && mutex.is_locked() {
            return None;
        }

        let deadlock_guard = Self::enter();

        if let Some(guard) = mutex.try_lock() {
            Some((guard, deadlock_guard))
        } else {
            None
        }
    }

    #[inline]
    fn lock_mutex<'a, T>(
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
