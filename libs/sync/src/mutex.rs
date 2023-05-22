use crate::{spin, MutexFlavor};
use core::{
    fmt,
    mem::ManuallyDrop,
    ops::{Deref, DerefMut},
};

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
            data: &self.data,
            _guard: guard,
        }
    }

    /// Attempts to acquire this mutex.
    ///
    /// This method may spuriously fail.
    #[inline]
    pub fn try_lock(&self) -> Option<MutexGuard<'_, T, F>> {
        F::try_lock(&self.inner, &self.data).map(|(inner, guard)| MutexGuard {
            inner: ManuallyDrop::new(inner),
            data: &self.data,
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
    data: &'a F::LockData,
    _guard: F::Guard,
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
        F::post_unlock(self.data);
    }
}
