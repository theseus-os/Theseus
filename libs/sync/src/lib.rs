#![no_std]

pub mod mutex;

pub use lock_api::{GuardNoSend, GuardSend};
pub use mutex::{Mutex, MutexGuard};

pub unsafe trait Flavour {
    const INIT: Self::LockData;

    type LockData;

    type GuardMarker;

    fn mutex_lock(mutex: &mutex::RawMutex<Self>)
    where
        Self: Sized;

    fn post_unlock(mutex: &mutex::RawMutex<Self>)
    where
        Self: Sized;
}

pub trait DeadlockPrevention {
    type GuardMarker;

    fn enter();

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
