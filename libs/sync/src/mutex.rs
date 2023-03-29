use crate::{DeadlockPrevention, Flavour};
use core::{
    marker::PhantomData,
    mem::ManuallyDrop,
    ops::{Deref, DerefMut},
    sync::atomic::{AtomicBool, Ordering},
};

pub struct Mutex<F, T>
where
    F: Flavour,
{
    inner: SpinMutex<F::DeadlockPrevention, T>,
    data: F::LockData,
    // To propagate !Send + !Sync bounds.
    _phantom: PhantomData<F>,
}

impl<F, T> Mutex<F, T>
where
    F: Flavour,
{
    pub const fn new(value: T) -> Self {
        Self {
            inner: SpinMutex::new(value),
            data: F::INIT,
            _phantom: PhantomData,
        }
    }

    pub fn into_inner(self) -> T {
        self.inner.into_inner()
    }

    pub fn lock(&self) -> MutexGuard<'_, F, T> {
        MutexGuard {
            inner: ManuallyDrop::new(self.inner.lock()),
            data: &self.data,
        }
    }

    pub fn try_lock(&self) -> Option<MutexGuard<'_, F, T>> {
        // TODO: Weak cmpxchg?
        self.inner.try_lock().map(|guard| MutexGuard {
            inner: ManuallyDrop::new(guard),
            data: &self.data,
        })
    }

    pub fn get_mut(&mut self) -> &mut T {
        self.inner.get_mut()
    }

    pub fn is_locked(&self) -> bool {
        self.inner.is_locked()
    }
}

pub struct MutexGuard<'a, F, T>
where
    F: Flavour,
{
    inner: ManuallyDrop<SpinMutexGuard<'a, F::DeadlockPrevention, T>>,
    data: &'a F::LockData,
}

impl<'a, F, T> Deref for MutexGuard<'a, F, T>
where
    F: Flavour,
{
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.inner.deref()
    }
}

impl<'a, F, T> DerefMut for MutexGuard<'a, F, T>
where
    F: Flavour,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.inner.deref_mut()
    }
}

impl<'a, F, T> Drop for MutexGuard<'a, F, T>
where
    F: Flavour,
{
    fn drop(&mut self) {
        unsafe { ManuallyDrop::drop(&mut self.inner) };
        F::post_unlock(self.data);
    }
}

pub type SpinMutex<P, T> = lock_api::Mutex<RawMutex<P>, T>;
pub type SpinMutexGuard<'a, P, T> = lock_api::MutexGuard<'a, RawMutex<P>, T>;

pub struct RawMutex<P>
where
    P: DeadlockPrevention,
{
    lock: AtomicBool,
    _phantom: PhantomData<P>,
}

unsafe impl<P> lock_api::RawMutex for RawMutex<P>
where
    P: DeadlockPrevention,
{
    const INIT: Self = Self {
        lock: AtomicBool::new(false),
        _phantom: PhantomData,
    };

    type GuardMarker = ();

    #[inline]
    fn lock(&self) {
        P::enter();
        while !self.try_lock_weak() {
            P::exit();
            while self.is_locked() {
                core::hint::spin_loop();
            }
            P::enter();
        }
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
    }

    #[inline]
    fn is_locked(&self) -> bool {
        self.lock.load(Ordering::Relaxed)
    }
}

impl<P> RawMutex<P>
where
    P: DeadlockPrevention,
{
    /// Tries to lock the mutex using [`AtomicBool::compare_exchange_weak`].
    #[inline]
    pub fn try_lock_weak(&self) -> bool {
        self.lock
            .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
    }
}
