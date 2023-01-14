use crate::Flavour;
use core::sync::atomic::{AtomicBool, Ordering};

pub type Mutex<F, T> = lock_api::Mutex<RawMutex<F>, T>;
pub type MutexGuard<'a, F, T> = lock_api::MutexGuard<'a, RawMutex<F>, T>;

pub struct RawMutex<T>
where
    T: Flavour,
{
    lock: AtomicBool,
    pub data: T::LockData,
}

unsafe impl<T> lock_api::RawMutex for RawMutex<T>
where
    T: Flavour,
{
    const INIT: Self = Self {
        lock: AtomicBool::new(false),
        data: T::INIT,
    };

    type GuardMarker = T::GuardMarker;

    #[inline]
    fn lock(&self) {
        T::mutex_lock(self);
    }

    #[inline]
    fn try_lock(&self) -> bool {
        self.lock
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
    }

    #[inline]
    unsafe fn unlock(&self) {
        self.lock.store(false, Ordering::Release);
        T::post_unlock(self);
    }

    #[inline]
    fn is_locked(&self) -> bool {
        self.lock.load(Ordering::Relaxed)
    }
}

impl<T> RawMutex<T>
where
    T: Flavour,
{
    /// Tries to lock the mutex using [`AtomicBool::compare_exchange_weak`].
    #[inline]
    pub fn try_lock_weak(&self) -> bool {
        self.lock
            .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
    }
}
