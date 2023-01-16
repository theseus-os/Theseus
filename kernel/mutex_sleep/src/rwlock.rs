// TODO: add documentation to each unsafe block, laying out all the conditions under which it's safe or unsafe to use it.
#![allow(clippy::missing_safety_doc)]

use core::fmt;
use core::ops::{Deref, DerefMut};
use spin::{RwLock, RwLockReadGuard, RwLockWriteGuard};
use wait_queue::WaitQueue;
use lockable::{Lockable, LockableSized};

/// A multi-reader, single-writer mutual exclusion wrapper that puts a `Task` to sleep
/// while waiting for the lock to become available. 
/// 
/// The behavior of this read-write lock is defined by the underlying [`spin::RwLock`];
/// 
/// A sleeping `Task` has a "blocked" runstate, meaning that it will not be scheduled in. 
/// Once the lock becomes available, `Task`s that are sleeping while waiting for the lock
/// will be notified (woken up) so they can attempt to acquire the lock again.
pub struct RwLockSleep<T: ?Sized> {
    queue: WaitQueue,
    rwlock: RwLock<T>,
}

/// A guard that allows the locked data to be immutably accessed,
/// during which mutual exclusion is guaranteed.
///
/// When the guard falls out of scope, the lock will be automatically released,
/// which then notifies any `Task`s waiting on the lock.
pub struct RwLockSleepReadGuard<'a, T: ?Sized + 'a> {
    guard: RwLockReadGuard<'a, T>,
    queue: &'a WaitQueue,
}

/// A guard that allows the locked data to be mutably accessed,
/// during which mutual exclusion is guaranteed.
///
/// When the guard falls out of scope, the lock will be automatically released,
/// which then notifies any `Task`s waiting on the lock.
pub struct RwLockSleepWriteGuard<'a, T: ?Sized + 'a> {
    guard: RwLockWriteGuard<'a, T>,
    queue: &'a WaitQueue,
}

// Same unsafe impls as `std::sync::RwLock`
unsafe impl<T: ?Sized + Send> Send for RwLockSleep<T> {}
unsafe impl<T: ?Sized + Send + Sync> Sync for RwLockSleep<T> {}

impl<T> RwLockSleep<T> {
    /// Creates a new lock wrapping the supplied data.
    ///
    // NOTE: const fn is currently disabled because the inner WaitQueue
    // is a VecDeque, which isn't statically initializable. 
    // When we switch to a different type, we can offer this as a const fn again.
    // pub const fn new (data: T) -> RwLockSleep<T> {
    pub fn new(data: T) -> RwLockSleep<T> {
        RwLockSleep {
            rwlock: RwLock::new(data),
            queue: WaitQueue::new(),
        }
    }

    /// Consumes this `RwLockSleep`, returning the underlying data.
    pub fn into_inner(self) -> T {
        self.rwlock.into_inner()
    }
}

impl<T: ?Sized> RwLockSleep<T> {
    /// Returns `true` if the lock is currently held.
    ///
    /// This function provides no synchronization guarantees and so its result should be considered 'out of date'
    /// the instant it is called. Do not use it for synchronization purposes. However, it may be useful as a heuristic.
    #[inline(always)]
    pub fn is_locked(&self) -> bool {
        self.rwlock.is_locked()
    }

    /// Locks this `RwLockSleep` with shared read (immutable) access.
    /// 
    /// If a writer has already acquired this lock, this function will put the current `Task`
    /// to sleep until the writer `Task` releases the lock, at which point this `Task` 
    /// will be woken up and attempt to acquire the read-only lock again.
    ///
    /// There may be other readers currently inside the lock when this function returns.
    /// This does not provide any guarantees with respect to the ordering 
    /// of whether contentious readers or writers will acquire the lock first.
    ///
    /// Returns an RAII guard which will release this task's shared access upon being dropped.
    ///
    /// ```
    /// let mylock = RwLockSleep::new(0);
    /// {
    ///     let mut data = mylock.read();
    ///     // The lock is now locked and the data can be read
    ///     println!("{}", *data);
    ///     // The lock is dropped and interrupts are restored to their prior state
    /// }
    /// ```
    pub fn read(&self) -> Result<RwLockSleepReadGuard<T>, &'static str> {
        // Fast path: check for the uncontended case.
        if let Some(guard) = self.try_read() {
            return Ok(guard);
        }
        // Slow path if already locked elsewhere: wait until we obtain the lock.
        self.queue
            .wait_until(&|| self.try_read())
            .map_err(|_| "failed to add current task to waitqueue")
    }

    /// Attempt to acquire this lock with shared read (immutable) access.
    ///
    /// This function is the same as [`RwLockSleep::read`] but will never block,
    /// returning immediately regardless of whether the lock has been acquired.
    ///
    /// ```
    /// let mylock = RwLockSleep::new(0);
    /// {
    ///     match mylock.try_read() {
    ///         Some(data) => {
    ///             // The lock is now locked and the data can be read
    ///             println!("{}", *data);
    ///             // The lock is dropped
    ///         },
    ///         None => (), // failed, another task holds the writer lock
    ///     };
    /// }
    /// ```
    pub fn try_read(&self) -> Option<RwLockSleepReadGuard<T>> {
        self.rwlock.try_read().map(|spinlock_guard| 
            RwLockSleepReadGuard {
                guard: spinlock_guard,
                queue: &self.queue
            }
        )
    }

    /// Return the number of readers that currently hold the lock (including upgradable readers).
    ///
    /// This function provides no synchronization guarantees and so its result should be considered 'out of date'
    /// the instant it is called. Do not use it for synchronization purposes. However, it may be useful as a heuristic.
    pub fn reader_count(&self) -> usize {
        self.rwlock.reader_count()
    }

    /// Return the number of writers that currently hold the lock.
    ///
    /// Because [`RwLockSleep`] guarantees exclusive mutable access, this function may only return either `0` or `1`.
    ///
    /// This function provides no synchronization guarantees and so its result should be considered 'out of date'
    /// the instant it is called. Do not use it for synchronization purposes. However, it may be useful as a heuristic.
    pub fn writer_count(&self) -> usize {
        self.rwlock.writer_count()
    }

    /// Force decrement the reader count.
    ///
    /// This is *extremely* unsafe if there are outstanding `RwLockSleepReadGuard`s
    /// live, or if called more times than `read` has been called, but can be
    /// useful in FFI contexts where the caller doesn't know how to deal with
    /// RAII.
    pub unsafe fn force_read_decrement(&self) {
        self.rwlock.force_read_decrement();
    }

    /// Force unlock exclusive write access.
    ///
    /// This is *extremely* unsafe if there are outstanding `RwLockSleepWriteGuard`s
    /// live, or if called when there are current readers, but can be useful in
    /// FFI contexts where the caller doesn't know how to deal with RAII.
    pub unsafe fn force_write_unlock(&self) {
        self.rwlock.force_write_unlock();
    }

    /// Locks this `RwLockSleep` with exclusive write (mutable) access.
    /// 
    /// If another writer or reader has already acquired this lock, this function will
    /// put the current `Task` to sleep until all other readers and the other writer `Task`
    /// release their locks, at which point this `Task` will be woken up and
    /// attempt to acquire the write lock again.
    ///
    /// This does not provide any guarantees with respect to the ordering 
    /// of whether contentious readers or writers will acquire the lock first.
    ///
    /// Returns an RAII guard which will release this task's exclusive access upon being dropped.
    ///
    /// ```
    /// let mylock = RwLockSleep::new(0);
    /// {
    ///     let mut data = mylock.write();
    ///     // The lock is now locked and the data can be written
    ///     *data += 1;
    ///     // The lock is dropped
    /// }
    /// ```
    pub fn write(&self) -> Result<RwLockSleepWriteGuard<T>, &'static str> {
        // Fast path: check for the uncontended case.
        if let Some(guard) = self.try_write() {
            return Ok(guard);
        }
        // Slow path if already locked elsewhere: wait until we obtain the write lock.
        self.queue
            .wait_until(&|| self.try_write())
            .map_err(|_| "failed to add current task to waitqueue")
    }

    /// Attempt to acquire this lock with exclusive write (mutable) access.
    ///
    /// This function is the same as [`RwLockSleep::write`] but will never block,
    /// returning immediately regardless of whether the lock has been acquired.
    ///
    /// ```
    /// let mylock = RwLockSleep::new(0);
    /// {
    ///     match mylock.try_write() {
    ///         Some(mut data) => {
    ///             // The lock is now locked and the data can be written
    ///             *data += 1;
    ///             // The lock is implicitly dropped
    ///         },
    ///         None => (), // failed, another task holds the writer lock
    ///     };
    /// }
    /// ```
    pub fn try_write(&self) -> Option<RwLockSleepWriteGuard<T>> {
        self.rwlock.try_write().map(|spinlock_guard|
            RwLockSleepWriteGuard {
                guard: spinlock_guard,
                queue: &self.queue,
            }
        )
    }

    /// Returns a mutable reference to the underlying data.
    ///
    /// Since this call borrows the [`RwLockSleep`] mutably, and a mutable reference is guaranteed to be exclusive in Rust,
    /// no actual locking needs to take place -- the mutable borrow statically guarantees no locks exist. As such,
    /// this is a 'zero-cost' operation.
    ///
    /// # Example
    ///
    /// ```
    /// let mut lock = RwLockSleep::new(0);
    /// *lock.get_mut() = 10;
    /// assert_eq!(*lock.lock(), 10);
    /// ```
    pub fn get_mut(&mut self) -> &mut T {
        self.rwlock.get_mut()
    }
}

impl<T: ?Sized + fmt::Debug> fmt::Debug for RwLockSleep<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.rwlock.try_read() {
            Some(guard) => write!(f, "RwLockSleep {{ data: {:?} }}", &*guard),
            None => write!(f, "RwLockSleep {{ <locked> }}"),
        }
    }
}

impl<T: ?Sized + Default> Default for RwLockSleep<T> {
    fn default() -> RwLockSleep<T> {
        RwLockSleep::new(Default::default())
    }
}

impl<'rwlock, T: ?Sized> Deref for RwLockSleepReadGuard<'rwlock, T> {
    type Target = T;

    fn deref(&self) -> &T { 
       &self.guard
    }
}

impl<'rwlock, T: ?Sized> Deref for RwLockSleepWriteGuard<'rwlock, T> {
    type Target = T;

    fn deref(&self) -> &T { 
        &self.guard
    }
}

impl<'rwlock, T: ?Sized> DerefMut for RwLockSleepWriteGuard<'rwlock, T> {
    fn deref_mut(&mut self) -> &mut T { 
        &mut self.guard
    }
}


impl<'rwlock, T: ?Sized> Drop for RwLockSleepReadGuard<'rwlock, T> {
    fn drop(&mut self) {
        // Notify a task on the waitqueue that the lock is released,
        // which occurs automatically when the inner `guard` is dropped after this method executes.
        self.queue.notify_one();
    }
}

impl<'rwlock, T: ?Sized> Drop for RwLockSleepWriteGuard<'rwlock, T> {
    fn drop(&mut self) {
        // Notify a task on the waitqueue that the lock is released,
        // which occurs automatically when the inner `guard` is dropped after this method executes.
        self.queue.notify_one();
    }
}

/// Implement `Lockable` for [`RwLockSleep`].
/// Because [`RwLockSleep::read()`] and [`RwLockSleep::write()`] return `Result`s and may fail,
/// the [`Lockable::lock()`] and `lock_mut()` functions internally `unwrap` those `Result`s.
impl<'t, T> Lockable<'t, T> for RwLockSleep<T> where T: 't + ?Sized {
    type Guard = RwLockSleepReadGuard<'t, T>;
    type GuardMut = RwLockSleepWriteGuard<'t, T>;

    fn lock(&'t self) -> Self::Guard { self.read().unwrap() }
    fn try_lock(&'t self) -> Option<Self::Guard> { self.try_read() }
    fn lock_mut(&'t self) -> Self::GuardMut { self.write().unwrap() }
    fn try_lock_mut(&'t self) -> Option<Self::GuardMut> { self.try_write() }
    fn is_locked(&self) -> bool { self.is_locked() }
    fn get_mut(&'t mut self) -> &mut T { self.get_mut() }
}
/// Implement `LockableSized` for [`RwLockSleep`].
impl<'t, T> LockableSized<'t, T> for RwLockSleep<T> where T: 't + Sized {
    fn into_inner(self) -> T { self.into_inner() }
}
