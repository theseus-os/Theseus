use crate::spin;
use core::{
    fmt,
    mem::ManuallyDrop,
    ops::{Deref, DerefMut},
};

pub trait MutexFlavor {
    /// Initial state of the additional data.
    const INIT: Self::LockData;

    /// Additional data stored in the lock.
    type LockData;

    /// Additional guard stored in the synchronisation guards.
    type Guard;

    /// Tries to acquire the given mutex.
    fn try_lock<'a, T>(
        mutex: &'a spin::Mutex<T>,
        data: &'a Self::LockData,
    ) -> Option<(spin::MutexGuard<'a, T>, Self::Guard)>;

    /// Acquires the given mutex.
    fn lock<'a, T>(
        mutex: &'a spin::Mutex<T>,
        data: &'a Self::LockData,
    ) -> (spin::MutexGuard<'a, T>, Self::Guard);

    /// Performs any necessary actions after unlocking the mutex.
    fn post_unlock(data: &Self::LockData);
}

/// A mutual exclusion primitive.
pub struct Mutex<T, F>
where
    F: MutexFlavor,
{
    inner: spin::Mutex<T>,
    data: F::LockData,
}

impl<T, F> Mutex<T, F>
where
    F: MutexFlavor,
{
    /// Creates a new mutex.
    #[inline]
    pub const fn new(value: T) -> Self {
        Self {
            inner: spin::Mutex::new(value),
            data: F::INIT,
        }
    }

    /// Consumes this mutex, returning the underlying data.
    #[inline]
    pub fn into_inner(self) -> T {
        self.inner.into_inner()
    }

    /// Returns a mutable reference to the underlying data.
    #[inline]
    pub fn get_mut(&mut self) -> &mut T {
        self.inner.get_mut()
    }

    /// Acquires this mutex.
    #[inline]
    pub fn lock(&self) -> MutexGuard<'_, T, F> {
        let (inner, guard) = F::lock(&self.inner, &self.data);

        MutexGuard {
            inner: ManuallyDrop::new(inner),
            lock: self,
            _guard: guard,
        }
    }

    /// Attempts to acquire this mutex.
    #[inline]
    pub fn try_lock(&self) -> Option<MutexGuard<'_, T, F>> {
        F::try_lock(&self.inner, &self.data).map(|(inner, guard)| MutexGuard {
            inner: ManuallyDrop::new(inner),
            lock: self,
            _guard: guard,
        })
    }

    /// Checks whether the mutex is currently locked.
    #[inline]
    pub fn is_locked(&self) -> bool {
        self.inner.is_locked()
    }
}

impl<T, F> fmt::Debug for Mutex<T, F>
where
    T: fmt::Debug,
    F: MutexFlavor,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut d = f.debug_struct("Mutex");
        match self.try_lock() {
            Some(guard) => {
                d.field("data", &&*guard);
            }
            None => {
                struct LockedPlaceholder;
                impl fmt::Debug for LockedPlaceholder {
                    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                        f.write_str("<locked>")
                    }
                }
                d.field("data", &LockedPlaceholder);
            }
        }
        d.finish_non_exhaustive()
    }
}

/// A RAII implementation of a "scoped lock" of a mutex.
///
/// When this structure is dropped, the lock will be unlocked.
#[derive(Debug)]
pub struct MutexGuard<'a, T, F>
where
    F: MutexFlavor,
{
    inner: ManuallyDrop<spin::MutexGuard<'a, T>>,
    lock: &'a Mutex<T, F>,
    _guard: F::Guard,
}

impl<'a, T, F> MutexGuard<'a, T, F>
where
    F: MutexFlavor,
{
    /// Leaks the guard, returning a mutable reference to the underlying data.
    ///
    /// This function will permanently lock the associated mutex.
    #[inline]
    pub fn leak(mut self) -> &'a mut T {
        // SAFETY: We forget self immediately after, so self.inner is never used again.
        let inner = unsafe { core::ptr::read(&mut self.inner) };
        core::mem::forget(self);
        spin_rs::mutex::SpinMutexGuard::<_>::leak(ManuallyDrop::into_inner(inner))
    }

    #[doc(hidden)]
    pub fn mutex(&self) -> &'a Mutex<T, F> {
        self.lock
    }
}

impl<'a, T, F> Deref for MutexGuard<'a, T, F>
where
    F: MutexFlavor,
{
    type Target = T;

    #[inline]
    fn deref(&self) -> &Self::Target {
        self.inner.deref()
    }
}

impl<'a, T, F> DerefMut for MutexGuard<'a, T, F>
where
    F: MutexFlavor,
{
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.inner.deref_mut()
    }
}

impl<'a, T, F> Drop for MutexGuard<'a, T, F>
where
    F: MutexFlavor,
{
    #[inline]
    fn drop(&mut self) {
        unsafe { ManuallyDrop::drop(&mut self.inner) };
        F::post_unlock(&self.lock.data);
    }
}
