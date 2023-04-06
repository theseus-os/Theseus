use crate::Flavour;
use core::{
    cell::UnsafeCell,
    fmt,
    mem::ManuallyDrop,
    ops::{Deref, DerefMut},
    sync::atomic::{AtomicBool, Ordering},
};

/// A mutual exclusion primitive.
pub struct Mutex<F, T>
where
    T: ?Sized,
    F: Flavour,
{
    data: F::LockData,
    inner: SpinMutex<T>,
}

impl<F, T> Mutex<F, T>
where
    F: Flavour,
{
    /// Creates a new mutex.
    pub const fn new(value: T) -> Self {
        Self {
            inner: SpinMutex::new(value),
            data: F::INIT,
        }
    }

    /// Consumes this mutex, returning the underlying data.
    pub fn into_inner(self) -> T {
        self.inner.into_inner()
    }

    /// Acquires this mutex.
    pub fn lock(&self) -> MutexGuard<'_, F, T> {
        let (inner, guard) = F::lock_mutex(&self.inner, &self.data);

        MutexGuard {
            inner: ManuallyDrop::new(inner),
            data: &self.data,
            _guard: guard,
        }
    }

    /// Attempts to acquire this mutex.
    pub fn try_lock(&self) -> Option<MutexGuard<'_, F, T>> {
        let (inner, guard) = F::try_lock_mutex(&self.inner, &self.data)?;

        Some(MutexGuard {
            inner: ManuallyDrop::new(inner),
            data: &self.data,
            _guard: guard,
        })
    }

    /// Returns a mutable reference to the underlying data.
    pub fn get_mut(&mut self) -> &mut T {
        self.inner.get_mut()
    }

    /// Checks whether the mutex is currently locked.
    pub fn is_locked(&self) -> bool {
        self.inner.is_locked()
    }
}

impl<F, T> fmt::Debug for Mutex<F, T>
where
    F: Flavour,
    T: fmt::Debug,
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
pub struct MutexGuard<'a, F, T>
where
    F: Flavour,
{
    inner: ManuallyDrop<SpinMutexGuard<'a, T>>,
    data: &'a F::LockData,
    _guard: F::Guard,
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

// Types below are copied from spin except that try_lock_weak is exposed.

pub struct SpinMutex<T>
where
    T: ?Sized,
{
    lock: AtomicBool,
    data: UnsafeCell<T>,
}

impl<T> SpinMutex<T> {
    pub const fn new(data: T) -> Self {
        Self {
            lock: AtomicBool::new(false),
            data: UnsafeCell::new(data),
        }
    }

    pub fn get_mut(&mut self) -> &mut T {
        self.data.get_mut()
    }

    pub fn into_inner(self) -> T
    where
        T: Sized,
    {
        self.data.into_inner()
    }

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

    fn deref(&self) -> &Self::Target {
        unsafe { &*self.data }
    }
}

impl<'a, T> DerefMut for SpinMutexGuard<'a, T>
where
    T: ?Sized,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.data }
    }
}

impl<'a, T> Drop for SpinMutexGuard<'a, T>
where
    T: ?Sized,
{
    fn drop(&mut self) {
        self.lock.store(false, Ordering::Release);
    }
}

// Same unsafe impls as `std::sync::Mutex`.

unsafe impl<T: ?Sized + Send> Sync for SpinMutex<T> {}
unsafe impl<T: ?Sized + Send> Send for SpinMutex<T> {}

unsafe impl<T: ?Sized + Sync> Sync for SpinMutexGuard<'_, T> {}
unsafe impl<T: ?Sized + Send> Send for SpinMutexGuard<'_, T> {}
