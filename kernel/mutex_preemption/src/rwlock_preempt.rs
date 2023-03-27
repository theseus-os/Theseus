// TODO: add documentation to each unsafe block, laying out all the conditions under which it's safe or unsafe to use it.
#![allow(clippy::missing_safety_doc)]

use core::{fmt, ops::{Deref, DerefMut}};
use preemption::{PreemptionGuard, hold_preemption};
use spin::{RwLock, RwLockReadGuard, RwLockWriteGuard};
use lockable::{Lockable, LockableSized};

/// A multi-reader, single-writer mutual exclusion wrapper that ensures preemption
/// is disabled on the current CPU for as long as the lock guard is held.
/// 
/// The behavior of this read-write lock is defined by the underlying [`spin::RwLock`];
pub struct RwLockPreempt<T: ?Sized> {
    rwlock: RwLock<T>,
}

/// A guard that allows the locked data to be immutably accessed,
/// during which mutual exclusion is guaranteed.
///
/// When the guard falls out of scope, the lock will be automatically released,
/// and preemption will be re-enabled on the current CPU if this was the last guard
/// that was keeping it disabled.
pub struct RwLockPreemptReadGuard<'a, T: ?Sized + 'a> {
    guard: RwLockReadGuard<'a, T>,
    // `_preemption_guard` will be dropped after `guard`.
    // Rust guarantees that fields are dropped in the order of declaration.
    _preemption_guard: PreemptionGuard,
}

/// A guard that allows the locked data to be mutably accessed,
/// during which mutual exclusion is guaranteed.
///
/// When the guard falls out of scope, the lock will be automatically released,
/// and preemption will be re-enabled on the current CPU if this was the last guard
/// that was keeping it disabled.
pub struct RwLockPreemptWriteGuard<'a, T: ?Sized + 'a> {
    guard: RwLockWriteGuard<'a, T>,
    // `_preemption_guard` will be dropped after `guard`.
    // Rust guarantees that fields are dropped in the order of declaration.
    _preemption_guard: PreemptionGuard,
}

// Same unsafe impls as `std::sync::RwLock`
unsafe impl<T: ?Sized + Send> Send for RwLockPreempt<T> {}
unsafe impl<T: ?Sized + Send + Sync> Sync for RwLockPreempt<T> {}

impl<T> RwLockPreempt<T> {
    /// Creates a new lock wrapping the supplied data.
    ///
    pub const fn new (data: T) -> RwLockPreempt<T> {
        RwLockPreempt {
            rwlock: RwLock::new(data),
        }
    }

    /// Consumes this `RwLockPreempt`, returning the underlying data.
    pub fn into_inner(self) -> T {
        self.rwlock.into_inner()
    }
}

impl<T: ?Sized> RwLockPreempt<T> {
    /// Returns `true` if the lock is currently held.
    ///
    /// # Safety
    ///
    /// This function provides no synchronization guarantees and so its result should be considered 'out of date'
    /// the instant it is called. Do not use it for synchronization purposes. However, it may be useful as a heuristic.
    #[inline(always)]
    pub fn is_locked(&self) -> bool {
        self.rwlock.is_locked()
    }

    /// Locks this `RwLockPreempt` with shared read (immutable) access, blocking the current task
    /// until it can be acquired.
    /// 
    /// The calling task will be blocked until there are no more writers which
    /// hold the lock. There may be other readers currently inside the lock when
    /// this method returns. This method does not provide any guarantees with
    /// respect to the ordering of whether contentious readers or writers will
    /// acquire the lock first.
    ///
    /// Returns an RAII guard which will release this task's shared access
    /// once it is dropped, along with restoring interrupts. 
    ///
    /// ```
    /// let mylock = RwLockPreempt::new(0);
    /// {
    ///     let mut data = mylock.read();
    ///     // The lock is now locked, preemption is disabled, and the data can be read
    ///     println!("{}", *data);
    ///     // The lock is dropped and preemption is restored to its prior state
    /// }
    /// ```
    pub fn read(&self) -> RwLockPreemptReadGuard<T> {
        loop {
            if let Some(guard) = self.try_read() { return guard }
        }
    }

    /// Attempt to acquire this lock with shared read (immutable) access.
    ///
    /// This function is the same as [`RwLockPreempt::read`] but will never block,
    /// returning immediately regardless of whether the lock has been acquired.
    ///
    /// ```
    /// let mylock = RwLockPreempt::new(0);
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
    pub fn try_read(&self) -> Option<RwLockPreemptReadGuard<T>> {
        if self.rwlock.writer_count() > 0 { return None; }
        let _preemption_guard = hold_preemption();
        self.rwlock.try_read().map(|guard| RwLockPreemptReadGuard {
            guard,
            _preemption_guard,
        })
    }

    /// Return the number of readers that currently hold the lock (including upgradable readers).
    ///
    /// # Safety
    ///
    /// This function provides no synchronization guarantees and so its result should be considered 'out of date'
    /// the instant it is called. Do not use it for synchronization purposes. However, it may be useful as a heuristic.
    pub fn reader_count(&self) -> usize {
        self.rwlock.reader_count()
    }

    /// Return the number of writers that currently hold the lock.
    ///
    /// Because [`RwLockPreempt`] guarantees exclusive mutable access, this function may only return either `0` or `1`.
    ///
    /// # Safety
    ///
    /// This function provides no synchronization guarantees and so its result should be considered 'out of date'
    /// the instant it is called. Do not use it for synchronization purposes. However, it may be useful as a heuristic.
    pub fn writer_count(&self) -> usize {
        self.rwlock.writer_count()
    }

    /// Force decrement the reader count.
    ///
    /// This is *extremely* unsafe if there are outstanding `RwLockPreemptReadGuard`s
    /// live, or if called more times than `read` has been called, but can be
    /// useful in FFI contexts where the caller doesn't know how to deal with
    /// RAII.
    pub unsafe fn force_read_decrement(&self) {
        self.rwlock.force_read_decrement();
    }

    /// Force unlock exclusive write access.
    ///
    /// This is *extremely* unsafe if there are outstanding `RwLockPreemptWriteGuard`s
    /// live, or if called when there are current readers, but can be useful in
    /// FFI contexts where the caller doesn't know how to deal with RAII.
    pub unsafe fn force_write_unlock(&self) {
        self.rwlock.force_write_unlock();
    }

    /// Lock this `RwLockPreempt` with exclusive write access, blocking the current
    /// task until it can be acquired.
    ///
    /// This function will not return while other writers or other readers
    /// currently have access to the lock.
    ///
    /// Returns an RAII guard which will drop the write access of this lock
    /// when dropped.
    ///
    /// ```
    /// let mylock = RwLockPreempt::new(0);
    /// {
    ///     let mut data = mylock.write();
    ///     // The lock is now locked and the data can be written
    ///     *data += 1;
    ///     // The lock is dropped
    /// }
    /// ```
    pub fn write(&self) -> RwLockPreemptWriteGuard<T> {
        loop {
            if let Some(guard) = self.try_write() { return guard }
        }
    }

    /// Attempt to acquire this lock with exclusive write (mutable) access.
    ///
    /// This function is the same as [`RwLockPreempt::write`] but will never block,
    /// returning immediately regardless of whether the lock has been acquired.
    ///
    /// ```
    /// let mylock = RwLockPreempt::new(0);
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
    pub fn try_write(&self) -> Option<RwLockPreemptWriteGuard<T>> {
        if self.rwlock.writer_count() > 0 || self.rwlock.reader_count() > 0 {
            return None;
        }
        let _preemption_guard = hold_preemption();
        self.rwlock.try_write().map(|guard| RwLockPreemptWriteGuard {
            guard,
            _preemption_guard,
        })
    }

    /// Returns a mutable reference to the underlying data.
    ///
    /// Since this call borrows the [`RwLockPreempt`] mutably, and a mutable reference is guaranteed to be exclusive in Rust,
    /// no actual locking needs to take place -- the mutable borrow statically guarantees no locks exist. As such,
    /// this is a 'zero-cost' operation.
    ///
    /// # Example
    ///
    /// ```
    /// let mut lock = RwLockPreempt::new(0);
    /// *lock.get_mut() = 10;
    /// assert_eq!(*lock.lock(), 10);
    /// ```
    pub fn get_mut(&mut self) -> &mut T {
        self.rwlock.get_mut()
    }
}

impl<T: ?Sized + fmt::Debug> fmt::Debug for RwLockPreempt<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.rwlock.try_read() {
            Some(guard) => write!(f, "RwLockPreempt {{ data: {:?} }}", &*guard),
            None => write!(f, "RwLockPreempt {{ <locked> }}"),
        }
    }
}

impl<T: ?Sized + Default> Default for RwLockPreempt<T> {
    fn default() -> RwLockPreempt<T> {
        RwLockPreempt::new(Default::default())
    }
}

impl<'rwlock, T: ?Sized> Deref for RwLockPreemptReadGuard<'rwlock, T> {
    type Target = T;

    fn deref(&self) -> &T { 
       &self.guard 
    }
}

impl<'rwlock, T: ?Sized> Deref for RwLockPreemptWriteGuard<'rwlock, T> {
    type Target = T;

    fn deref(&self) -> &T { 
        &self.guard
    }
}

impl<'rwlock, T: ?Sized> DerefMut for RwLockPreemptWriteGuard<'rwlock, T> {
    fn deref_mut(&mut self) -> &mut T { 
        &mut self.guard
    }
}

/// Implement `Lockable` for [`RwLockPreempt`].
impl<'t, T> Lockable<'t, T> for RwLockPreempt<T> where T: 't + ?Sized {
    type Guard = RwLockPreemptReadGuard<'t, T>;
    type GuardMut = RwLockPreemptWriteGuard<'t, T>;

    fn lock(&'t self) -> Self::Guard { self.read() }
    fn try_lock(&'t self) -> Option<Self::Guard> { self.try_read() }
    fn lock_mut(&'t self) -> Self::GuardMut { self.write() }
    fn try_lock_mut(&'t self) -> Option<Self::GuardMut> { self.try_write() }
    fn is_locked(&self) -> bool { self.is_locked() }
    fn get_mut(&'t mut self) -> &mut T { self.get_mut() }
}
/// Implement `LockableSized` for [`RwLockPreempt`].
impl<'t, T> LockableSized<'t, T> for RwLockPreempt<T> where T: 't + Sized {
    fn into_inner(self) -> T { self.into_inner() }
}
