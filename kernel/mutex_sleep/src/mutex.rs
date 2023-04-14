use core::fmt;
use core::ops::{Deref, DerefMut};
use spin::{Mutex, MutexGuard};
use wait_queue::WaitQueue;
use lockable::{Lockable, LockableSized};

/// A mutual exclusion wrapper that puts a `Task` to sleep while waiting for the lock to become available. 
/// 
/// A sleeping `Task` has a "blocked" runstate, meaning that it will not be scheduled in. 
/// Once the lock becomes available, `Task`s that are sleeping while waiting for the lock
/// will be notified (woken up) so they can attempt to acquire the lock again.
pub struct MutexSleep<T: ?Sized> {
    queue: WaitQueue,
    lock: Mutex<T>,
}

/// A guard that allows the locked data to be accessed, during which mutual exclusion is guaranteed.
///
/// When the guard falls out of scope, the lock will be automatically released,
/// which then notifies any `Task`s waiting on the lock.
pub struct MutexSleepGuard<'a, T: ?Sized + 'a> {
    guard: MutexGuard<'a, T>,
    queue: &'a WaitQueue,
}

// Same unsafe impls as `std::sync::Mutex`
unsafe impl<T: ?Sized + Send> Send for MutexSleep<T> {}
unsafe impl<T: ?Sized + Send> Sync for MutexSleep<T> {}

impl<T> MutexSleep<T> {
    /// Creates a new lock wrapping the supplied data.
    ///
    // NOTE: const fn is currently disabled because the inner WaitQueue
    // is a VecDeque, which isn't statically initializable. 
    // When we switch to a different type, we can offer this as a const fn again.
    // pub const fn new (data: T) -> MutexSleep<T> {
    pub fn new(data: T) -> MutexSleep<T> {
        MutexSleep {
            lock: Mutex::new(data),
            queue: WaitQueue::new(),
        }
    }

    /// Consumes this `MutexSleep`, returning the underlying data.
    pub fn into_inner(self) -> T {
        self.lock.into_inner()
    }
}

impl<T: ?Sized> MutexSleep<T> {
    /// Returns `true` if the lock is currently held.
    ///
    /// # Safety
    ///
    /// This function provides no synchronization guarantees and so its result should be considered 'out of date'
    /// the instant it is called. Do not use it for synchronization purposes. However, it may be useful as a heuristic.
    #[inline(always)]
    pub fn is_locked(&self) -> bool {
        self.lock.is_locked()
    }

    /// Blocks until the lock is acquired by putting this `Task` to sleep 
    /// until another `Task` that has the lock releases it. 
    ///
    /// The returned guard may be dereferenced to access the protected data;
    /// the lock will be released when the returned guard falls out of scope and is dropped.
    pub fn lock(&self) -> Result<MutexSleepGuard<T>, &'static str> {
        // Fast path: check for the uncontended case.
        if let Some(guard) = self.try_lock() {
            return Ok(guard);
        }
        // Slow path if already locked elsewhere: wait until we obtain the lock.
        Ok(self.queue.wait_until(|| self.try_lock()))
    }

    /// Tries to lock the MutexSleep. If it is already locked, it will return `None`.
    /// Otherwise it returns a guard within `Some`.
    pub fn try_lock(&self) -> Option<MutexSleepGuard<T>> {
        self.lock.try_lock().map(|spinlock_guard| {
            MutexSleepGuard {
                guard: spinlock_guard,
                queue: &self.queue,
            }
        })
    }

    /// Returns a mutable reference to the underlying data.
    ///
    /// Since this call borrows the [`MutexSleep`] mutably, and a mutable reference is guaranteed to be exclusive in Rust,
    /// no actual locking needs to take place -- the mutable borrow statically guarantees no locks exist. As such,
    /// this is a 'zero-cost' operation.
    ///
    /// # Example
    ///
    /// ```
    /// let mut lock = MutexSleep::new(0);
    /// *lock.get_mut() = 10;
    /// assert_eq!(*lock.lock(), 10);
    /// ```
    #[inline(always)]
    pub fn get_mut(&mut self) -> &mut T {
        self.lock.get_mut()
    }
}

impl<T: ?Sized + fmt::Debug> fmt::Debug for MutexSleep<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.lock.try_lock() {
            Some(guard) => write!(f, "MutexSleep {{ data: {:?} }}", &*guard),
            None => write!(f, "MutexSleep {{ <locked> }}"),
        }
    }
}

impl<T: ?Sized + Default> Default for MutexSleep<T> {
    fn default() -> MutexSleep<T> {
        MutexSleep::new(Default::default())
    }
}

impl<'a, T: ?Sized> Deref for MutexSleepGuard<'a, T> {
    type Target = T;

    fn deref(&self) -> &T { 
        &self.guard
    }
}

impl<'a, T: ?Sized> DerefMut for MutexSleepGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut T { 
        &mut self.guard
    }
}


impl<'a, T: ?Sized> Drop for MutexSleepGuard<'a, T> {
    fn drop(&mut self) {
        // Notify a task on the waitqueue that the lock is released,
        // which occurs automatically when the inner `guard` is dropped after this method executes.
        self.queue.notify_one();
    }
}

/// Implement `Lockable` for [`MutexSleep`].
/// Because [`MutexSleep::lock()`] returns a `Result` and may fail,
/// the [`Lockable::lock()`] function internally `unwrap`s that `Result`.
impl<'t, T> Lockable<'t, T> for MutexSleep<T> where T: 't + ?Sized {
    type Guard = MutexSleepGuard<'t, T>;
    type GuardMut = Self::Guard;

    fn lock(&'t self) -> Self::Guard { self.lock().unwrap() }
    fn try_lock(&'t self) -> Option<Self::Guard> { self.try_lock() }
    fn lock_mut(&'t self) -> Self::GuardMut { self.lock().unwrap() }
    fn try_lock_mut(&'t self) -> Option<Self::GuardMut> { self.try_lock() }
    fn is_locked(&self) -> bool { self.is_locked() }
    fn get_mut(&'t mut self) -> &mut T { self.get_mut() }
}
/// Implement `LockableSized` for [`MutexSleep`].
impl<'t, T> LockableSized<'t, T> for MutexSleep<T> where T: 't + Sized {
    fn into_inner(self) -> T { self.into_inner() }
}
