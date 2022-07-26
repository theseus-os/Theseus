#![feature(must_not_suspend, negative_impls)]
#![no_std]

use core::{
    cell::UnsafeCell,
    fmt,
    ops::{Deref, DerefMut},
};
// use lockable::{Lockable, LockableSized};

/// A mutual exclusion primitive useful for protecting shared data
///
/// This mutex will block threads waiting for the lock to become available. The
/// mutex can be created via a [`new`] constructor. Each mutex has a type
/// parameter which represents the data that it is protecting. The data can only
/// be accessed through the RAII guards returned from [`lock`] and [`try_lock`],
/// which guarantees that the data is only ever accessed when the mutex is
/// locked.
///
/// [`new`]: Self::new
/// [`lock`]: Self::lock
/// [`try_lock`]: Self::try_lock
pub struct Mutex<T: ?Sized> {
    inner: raw_mutex::Mutex,
    data: UnsafeCell<T>,
}

// SAFETY: Same impl as `std::sync::Mutex`.
unsafe impl<T: ?Sized + Send> Send for Mutex<T> {}
// SAFETY: Same impl as `std::sync::Mutex`.
unsafe impl<T: ?Sized + Send> Sync for Mutex<T> {}

/// An RAII implementation of a "scoped lock" of a mutex. When this structure is
/// dropped (falls out of scope), the lock will be unlocked.
///
/// The data protected by the mutex can be accessed through this guard via its
/// [`Deref`] and [`DerefMut`] implementations.
///
/// This structure is created by the [`lock`] and [`try_lock`] methods on
/// [`Mutex`].
///
/// [`lock`]: Mutex::lock
/// [`try_lock`]: Mutex::try_lock
#[must_use = "if unused the Mutex will immediately unlock"]
#[must_not_suspend = "holding a MutexGuard across suspend \
                      points can cause deadlocks, delays, \
                      and cause Futures to not implement `Send`"]
#[clippy::has_significant_drop]
pub struct MutexGuard<'a, T: ?Sized + 'a> {
    lock: &'a Mutex<T>,
}

impl<T: ?Sized> !Send for MutexGuard<'_, T> {}
// SAFETY: Same impl as `std::sync::MutexGuard`.
unsafe impl<T: ?Sized + Sync> Sync for MutexGuard<'_, T> {}

impl<T> Mutex<T> {
    /// Creates a new mutex in an unlocked state ready for use.
    ///
    /// # Examples
    ///
    /// ```
    /// use sync::Mutex;
    ///
    /// let mutex = Mutex::new(0);
    /// ```
    // NOTE: const fn is currently disabled because the inner mutex contains a
    // VecDeque which isn't statically initializable. When we switch to a different
    // type, we can offer this as a const fn again.
    pub fn new(data: T) -> Self {
        Self {
            inner: raw_mutex::Mutex::new(),
            data: UnsafeCell::new(data),
        }
    }
}

impl<T: ?Sized> Mutex<T> {
    /// Acquires a mutex, blocking the current thread until it is able to do so.
    ///
    /// This function will block the local thread until it is available to
    /// acquire the mutex. Upon returning, the thread is the only thread
    /// with the lock held. An RAII guard is returned to allow scoped unlock
    /// of the lock. When the guard goes out of scope, the mutex will be
    /// unlocked.
    ///
    /// If locking a mutex in the thread which already holds the lock, this
    /// function will deadlock.
    pub fn lock(&self) -> MutexGuard<'_, T> {
        self.inner.lock();
        // SAFETY: We just locked the mutex.
        unsafe { MutexGuard::new(self) }
    }

    /// Attempts to acquire this lock.
    ///
    /// If the lock could not be acquired at this time, then [`None`] is
    /// returned. Otherwise, an RAII guard is returned. The lock will be
    /// unlocked when the guard is dropped.
    ///
    /// This function does not block.
    pub fn try_lock(&self) -> Option<MutexGuard<'_, T>> {
        if self.inner.try_lock() {
            // SAFETY: We just locked the mutex.
            Some(unsafe { MutexGuard::new(self) })
        } else {
            None
        }
    }

    /// Consumes this mutex, returning the underlying data.
    ///
    /// # Examples
    ///
    /// ```
    /// use sync::Mutex;
    ///
    /// let mutex = Mutex::new(0);
    /// assert_eq!(mutex.into_inner(), 0);
    /// ```
    pub fn into_inner(self) -> T
    where
        T: Sized,
    {
        self.data.into_inner()
    }

    /// Returns a mutable reference to the underlying data.
    ///
    /// Since this call borrows the [`Mutex`] mutably, and a mutable
    /// reference is guaranteed to be exclusive in Rust, no actual locking
    /// needs to take place -- the mutable borrow statically guarantees no locks
    /// exist. As such, this is a 'zero-cost' operation.
    ///
    /// # Example
    ///
    /// ```
    /// use sync::Mutex;
    ///
    /// let mut lock = Mutex::new(0);
    /// *lock.get_mut() = 10;
    /// assert_eq!(*lock.lock(), 10);
    /// ```
    #[inline(always)]
    pub fn get_mut(&mut self) -> &mut T {
        self.data.get_mut()
    }
}

impl<T> From<T> for Mutex<T> {
    /// Creates a new mutex in an unlocked state ready for use.
    /// This is equivalent to [`Mutex::new`].
    fn from(t: T) -> Self {
        Mutex::new(t)
    }
}

impl<T: ?Sized + Default> Default for Mutex<T> {
    fn default() -> Mutex<T> {
        Mutex::new(Default::default())
    }
}

impl<T: ?Sized + fmt::Debug> fmt::Debug for Mutex<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
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

impl<'mutex, T: ?Sized> MutexGuard<'mutex, T> {
    /// Create a new `MutexGuard` from a locked [`Mutex`].
    ///
    /// # Safety
    ///
    /// `lock` must be locked by the current thread.
    unsafe fn new(lock: &'mutex Mutex<T>) -> MutexGuard<'mutex, T> {
        MutexGuard { lock }
    }
}

impl<T: ?Sized> Deref for MutexGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        // SAFETY: self existing is proof that the mutex is locked by the current
        // thread.
        unsafe { &*self.lock.data.get() }
    }
}

impl<T: ?Sized> DerefMut for MutexGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        // SAFETY: self existing is proof that the mutex is locked by the current
        // thread.
        unsafe { &mut *self.lock.data.get() }
    }
}

impl<'a, T: ?Sized> Drop for MutexGuard<'a, T> {
    fn drop(&mut self) {
        // SAFETY: self existing is proof that the mutex is locked by the current
        // thread.
        unsafe { self.lock.inner.unlock() };
    }
}

impl<T: ?Sized + fmt::Debug> fmt::Debug for MutexGuard<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&**self, f)
    }
}

impl<T: ?Sized + fmt::Display> fmt::Display for MutexGuard<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        (**self).fmt(f)
    }
}

/// Acquire the raw mutex of the guard.
///
/// This function must only be used by the condition variable implementation.
#[doc(hidden)]
pub fn guard_lock<'a, T: ?Sized>(guard: &MutexGuard<'a, T>) -> &'a raw_mutex::Mutex {
    &guard.lock.inner
}

// impl<'t, T> Lockable<'t, T> for Mutex<T>
// where
//     T: 't + ?Sized,
// {
//     type Guard = MutexGuard<'t, T>;
//     type GuardMut = Self::Guard;

//     fn lock(&'t self) -> Self::Guard {
//         self.lock()
//     }
//     fn try_lock(&'t self) -> Option<Self::Guard> {
//         self.try_lock()
//     }
//     fn lock_mut(&'t self) -> Self::GuardMut {
//         self.lock()
//     }
//     fn try_lock_mut(&'t self) -> Option<Self::GuardMut> {
//         self.try_lock()
//     }
//     fn is_locked(&self) -> bool {
//         self.is_locked()
//     }
//     fn get_mut(&'t mut self) -> &mut T {
//         self.get_mut()
//     }
// }
// /// Implement `LockableSized` for [`Mutex`].
// impl<'t, T> LockableSized<'t, T> for Mutex<T>
// where
//     T: 't + Sized,
// {
//     fn into_inner(self) -> T {
//         self.into_inner()
//     }
// }
