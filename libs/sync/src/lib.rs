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

    /// Additional data stored on the lock.
    type LockData;

    type DeadlockPrevention: DeadlockPrevention;

    /// Acquires the given mutex.
    fn mutex_lock<'a, T>(
        mutex: &'a mutex::SpinMutex<Self::DeadlockPrevention, T>,
        data: &'a Self::LockData,
    ) -> mutex::SpinMutexGuard<'a, Self::DeadlockPrevention, T>
    where
        Self: Sized;

    /// Performs any necessary actions after unlocking the mutex.
    fn post_unlock(mutex: &Self::LockData)
    where
        Self: Sized;
}

/// A deadlock prevention method.
pub trait DeadlockPrevention {
    /// Performs any necessary actions prior to locking.
    fn enter();

    /// Performs any necessary actions after unlocking.
    fn exit();
}

impl<P> Flavour for P
where
    P: DeadlockPrevention,
{
    const INIT: Self::LockData = ();

    type LockData = ();

    type DeadlockPrevention = P;

    #[inline]
    fn mutex_lock<'a, T>(
        mutex: &'a mutex::SpinMutex<Self::DeadlockPrevention, T>,
        _: &'a Self::LockData,
    ) -> mutex::SpinMutexGuard<'a, Self::DeadlockPrevention, T> {
        mutex.lock()
    }

    #[inline]
    fn post_unlock(_: &Self::LockData) {}
}
