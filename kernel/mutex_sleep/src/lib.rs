//! A mutex that puts tasks to sleep while they wait for the lock. 

#![no_std]

extern crate spin;
extern crate owning_ref;
extern crate stable_deref_trait;
extern crate wait_queue;
extern crate task;

use core::fmt;
use core::ops::{Deref, DerefMut};
use spin::{Mutex, MutexGuard};
use owning_ref::{OwningRef, OwningRefMut};
use stable_deref_trait::StableDeref;
use wait_queue::WaitQueue;


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
unsafe impl<T: ?Sized + Send> Sync for MutexSleep<T> {}
unsafe impl<T: ?Sized + Send> Send for MutexSleep<T> {}

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
        self.queue
            .wait_until(&|| Ok(self.try_lock()))
            .map_err(|_| "failed to add current task to waitqueue")
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

    fn deref<'b>(&'b self) -> &'b T { 
        &*(self.guard) 
    }
}

impl<'a, T: ?Sized> DerefMut for MutexSleepGuard<'a, T> {
    fn deref_mut<'b>(&'b mut self) -> &'b mut T { 
        &mut *(self.guard)
    }
}


impl<'a, T: ?Sized> Drop for MutexSleepGuard<'a, T> {
    fn drop(&mut self) {
        // Notify a task on the waitqueue that the lock is released,
        // which occurs automatically when the inner `guard` is dropped after this method executes.
        self.queue.notify_one();
    }
}

// Implement the StableDeref trait for MutexSleep guards, just like it's implemented for Mutex guards
unsafe impl<'a, T: ?Sized> StableDeref for MutexSleepGuard<'a, T> {}

/// Typedef of a owning reference that uses a `MutexSleepGuard` as the owner.
pub type MutexSleepGuardRef<'a, T, U = T> = OwningRef<MutexSleepGuard<'a, T>, U>;
/// Typedef of a mutable owning reference that uses a `MutexSleepGuard` as the owner.
pub type MutexSleepGuardRefMut<'a, T, U = T> = OwningRefMut<MutexSleepGuard<'a, T>, U>;
