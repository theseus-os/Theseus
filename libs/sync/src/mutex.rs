use crate::Flavour;
use core::{
    cell::UnsafeCell,
    fmt,
    mem::ManuallyDrop,
    ops::{Deref, DerefMut},
    sync::atomic::{AtomicBool, Ordering},
};

/// A mutual exclusion primitive.
pub struct Mutex<T, F>
where
    F: Flavour,
{
    inner: SpinMutex<T>,
    data: F::LockData,
}

impl<T, F> Mutex<T, F>
where
    F: Flavour,
{
    /// Creates a new mutex.
    #[inline]
    pub const fn new(value: T) -> Self {
        Self {
            inner: SpinMutex::new(value),
            data: F::INIT,
        }
    }

    /// Consumes this mutex, returning the underlying data.
    #[inline]
    pub fn into_inner(self) -> T {
        self.inner.into_inner()
    }

    /// Acquires this mutex.
    #[inline]
    pub fn lock(&self) -> MutexGuard<'_, T, F> {
        let (inner, guard) = F::lock_mutex(&self.inner, &self.data);

        MutexGuard {
            inner: ManuallyDrop::new(inner),
            data: &self.data,
            _guard: guard,
        }
    }

    /// Attempts to acquire this mutex.
    #[inline]
    pub fn try_lock(&self) -> Option<MutexGuard<'_, T, F>> {
        let (inner, guard) = F::try_lock_mutex(&self.inner, &self.data)?;

        Some(MutexGuard {
            inner: ManuallyDrop::new(inner),
            data: &self.data,
            _guard: guard,
        })
    }

    /// Returns a mutable reference to the underlying data.
    #[inline]
    pub fn get_mut(&mut self) -> &mut T {
        self.inner.get_mut()
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
    F: Flavour,
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
    F: Flavour,
{
    inner: ManuallyDrop<SpinMutexGuard<'a, T>>,
    data: &'a F::LockData,
    _guard: F::Guard,
}

impl<'a, T, F> Deref for MutexGuard<'a, T, F>
where
    F: Flavour,
{
    type Target = T;

    #[inline]
    fn deref(&self) -> &Self::Target {
        self.inner.deref()
    }
}

impl<'a, T, F> DerefMut for MutexGuard<'a, T, F>
where
    F: Flavour,
{
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.inner.deref_mut()
    }
}

impl<'a, T, F> Drop for MutexGuard<'a, T, F>
where
    F: Flavour,
{
    #[inline]
    fn drop(&mut self) {
        unsafe { ManuallyDrop::drop(&mut self.inner) };
        F::post_unlock(self.data);
    }
}

// Types below are copied from spin except that try_lock_weak is exposed.

pub struct SpinMutex<T>
where
    T: ?Sized,
{
    lock: AtomicBool,
    data: UnsafeCell<T>,
}

impl<T> SpinMutex<T> {
    #[inline]
    pub const fn new(data: T) -> Self {
        Self {
            lock: AtomicBool::new(false),
            data: UnsafeCell::new(data),
        }
    }

    #[inline]
    pub fn get_mut(&mut self) -> &mut T {
        self.data.get_mut()
    }

    #[inline]
    pub fn into_inner(self) -> T
    where
        T: Sized,
    {
        self.data.into_inner()
    }

    #[inline]
    pub fn is_locked(&self) -> bool {
        self.lock.load(Ordering::Relaxed)
    }

    #[inline]
    pub fn try_lock(&self) -> Option<SpinMutexGuard<'_, T>> {
        self.lock
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .ok()?;

        Some(unsafe { self.guard() })
    }

    #[inline]
    pub fn try_lock_weak(&self) -> Option<SpinMutexGuard<'_, T>> {
        self.lock
            .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
            .ok()?;

        Some(unsafe { self.guard() })
    }

    #[inline]
    unsafe fn guard(&self) -> SpinMutexGuard<'_, T> {
        SpinMutexGuard {
            lock: &self.lock,
            data: unsafe { &mut *self.data.get() },
        }
    }
}

impl<T> fmt::Debug for SpinMutex<T>
where
    T: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut d = f.debug_struct("SpinMutex");
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

#[derive(Debug)]
pub struct SpinMutexGuard<'a, T>
where
    T: ?Sized,
{
    lock: &'a AtomicBool,
    data: *mut T,
}

impl<'a, T> Deref for SpinMutexGuard<'a, T>
where
    T: ?Sized,
{
    type Target = T;

    #[inline]
    fn deref(&self) -> &Self::Target {
        unsafe { &*self.data }
    }
}

impl<'a, T> DerefMut for SpinMutexGuard<'a, T>
where
    T: ?Sized,
{
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.data }
    }
}

impl<'a, T> Drop for SpinMutexGuard<'a, T>
where
    T: ?Sized,
{
    #[inline]
    fn drop(&mut self) {
        self.lock.store(false, Ordering::Release);
    }
}

// Same unsafe impls as `std::sync::Mutex`.

unsafe impl<T: ?Sized + Send> Sync for SpinMutex<T> {}
unsafe impl<T: ?Sized + Send> Send for SpinMutex<T> {}

unsafe impl<T: ?Sized + Sync> Sync for SpinMutexGuard<'_, T> {}
unsafe impl<T: ?Sized + Send> Send for SpinMutexGuard<'_, T> {}
