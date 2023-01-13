use crate::Flavour;
use core::sync::atomic::{AtomicBool, Ordering};

#[doc(hidden)]
pub struct RawMutex<T>
where
    T: Flavour,
{
    lock: AtomicBool,
    pub(crate) data: T::LockData,
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
        if self.try_lock_weak() {
            return;
        }

        T::mutex_slow_path(self);
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
    #[inline]
    pub(crate) fn try_lock_weak(&self) -> bool {
        self.lock
            .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
    }
}
