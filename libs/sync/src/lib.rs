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

pub use lock_api::{GuardNoSend, GuardSend};
pub use mutex::{Mutex, MutexGuard};

pub unsafe trait Flavour {
    /// Initial value for the lock data.
    const INIT: Self::LockData;

    /// Additional data stored on the lock.
    type LockData;

    /// Marker type to determine whether a lock guard should be send. Use either
    /// [`GuardSend`] or [`GuardNoSend`].
    type GuardMarker;

    /// Acquires the given mutex.
    fn mutex_lock(mutex: &mutex::RawMutex<Self>)
    where
        Self: Sized;

    /// Performs any necessary actions after unlocking the mutex.
    fn post_unlock(mutex: &mutex::RawMutex<Self>)
    where
        Self: Sized;
}

pub trait DeadlockPrevention {
    /// Marker type to determine whether a lock guard should be send. Use either
    /// [`GuardSend`] or [`GuardNoSend`].
    type GuardMarker;

    /// Performs any necessary actions prior to locking.
    fn enter();

    /// Performs any necessary actions after unlocking.
    fn exit();
}

unsafe impl<T> Flavour for T
where
    T: DeadlockPrevention,
{
    const INIT: Self::LockData = ();

    type LockData = ();

    type GuardMarker = <Self as DeadlockPrevention>::GuardMarker;

    #[inline]
    fn mutex_lock(mutex: &mutex::RawMutex<Self>) {
        use lock_api::RawMutex;

        T::enter();
        while !mutex.try_lock_weak() {
            T::exit();
            while mutex.is_locked() {
                core::hint::spin_loop();
            }
            T::enter();
        }
    }

    #[inline]
    fn post_unlock(_: &mutex::RawMutex<Self>) {
        T::exit();
    }
}
