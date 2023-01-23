use core::{fmt, ops::{Deref, DerefMut}};
use preemption::{PreemptionGuard, hold_preemption};
use spin::{Mutex, MutexGuard};
use lockable::{Lockable, LockableSized};

/// A mutual exclusion wrapper based on [`spin::Mutex`] that ensures preemption
/// is disabled on the current CPU for as long as the lock guard is held.
pub struct MutexPreempt<T: ?Sized> {
    lock: Mutex<T>,
}

/// A guard that allows the locked data to be accessed, during which mutual exclusion is guaranteed.
///
/// When the guard falls out of scope, the lock will be automatically released,
/// which then notifies any `Task`s waiting on the lock.
pub struct MutexPreemptGuard<'a, T: ?Sized + 'a> {
    guard: MutexGuard<'a, T>,
    // `_preemption_guard` will be dropped after `guard`.
    // Rust guarantees that fields are dropped in the order of declaration.
    _preemption_guard: PreemptionGuard,
}

// Same unsafe impls as `std::sync::Mutex`
unsafe impl<T: ?Sized + Send> Sync for MutexPreempt<T> {}
unsafe impl<T: ?Sized + Send> Send for MutexPreempt<T> {}

impl<T> MutexPreempt<T> {
    /// Creates a new lock wrapping the supplied data.
    pub const fn new(data: T) -> MutexPreempt<T> {
        MutexPreempt {
            lock: Mutex::new(data),
        }
    }

    /// Consumes this MutexPreempt, returning the underlying data.
    #[inline(always)]
    pub fn into_inner(self) -> T {
        self.lock.into_inner()
    }
}

impl<T: ?Sized> MutexPreempt<T> {
    /// Spins until the lock can be acquired, upon which preemption is disabled 
    /// for the duration that the returned guard is held.
    ///
    /// The returned value may be dereferenced for data access
    /// and the lock will be dropped when the guard falls out of scope.
    ///
    /// ```
    /// let mylock = irq_safety::MutexPreempt::new(0);
    /// {
    ///     let mut data = mylock.lock();
    ///     // The lock is now locked and the data can be accessed
    ///     *data += 1;
    ///     // The lock is implicitly dropped
    /// }
    ///
    /// ```
    #[inline(always)]
    pub fn lock(&self) -> MutexPreemptGuard<T> {
        loop {
            if let Some(guard) = self.try_lock() {
                return guard;
            }
        }
    }

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

    /// Force unlock the spinlock.
    ///
    /// # Safety
    /// This is *extremely* unsafe if the lock is not held by the current
    /// thread. However, this can be useful in some instances for exposing the
    /// lock to FFI that doesn't know how to deal with RAII.
    ///
    /// If the lock isn't held, this is a no-op.
    pub unsafe fn force_unlock(&self) {
        self.lock.force_unlock()
    }

    /// Tries to lock the MutexPreempt. If it is already locked, it will return None. Otherwise it returns
    /// a guard within Some.
    #[inline(always)]
    pub fn try_lock(&self) -> Option<MutexPreemptGuard<T>> {
        if self.lock.is_locked() { return None; }
        let _preemption_guard = hold_preemption();
        self.lock.try_lock().map(|guard| MutexPreemptGuard {
            guard,
            _preemption_guard,
        })
    }

    /// Returns a mutable reference to the underlying data.
    ///
    /// Since this call borrows the [`MutexPreempt`] mutably, and a mutable reference is guaranteed to be exclusive in Rust,
    /// no actual locking needs to take place -- the mutable borrow statically guarantees no locks exist. As such,
    /// this is a 'zero-cost' operation.
    ///
    /// # Example
    ///
    /// ```
    /// let mut lock = irq_safety::MutexPreempt::new(0);
    /// *lock.get_mut() = 10;
    /// assert_eq!(*lock.lock(), 10);
    /// ```
    #[inline(always)]
    pub fn get_mut(&mut self) -> &mut T {
        self.lock.get_mut()
    }
}

impl<T: ?Sized + fmt::Debug> fmt::Debug for MutexPreempt<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.lock.try_lock() {
            Some(guard) => write!(f, "MutexPreempt {{ data: {:?} }}", &*guard),
            None => write!(f, "MutexPreempt {{ <locked> }}"),
        }
    }
}

impl<T: ?Sized + Default> Default for MutexPreempt<T> {
    fn default() -> MutexPreempt<T> {
        MutexPreempt::new(Default::default())
    }
}

impl<'a, T: ?Sized> Deref for MutexPreemptGuard<'a, T> {
    type Target = T;

    fn deref(&self) -> &T { 
        &self.guard 
    }
}

impl<'a, T: ?Sized> DerefMut for MutexPreemptGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut T { 
        &mut self.guard
    }
}

/// Implement `Lockable` for [`MutexPreempt`].
impl<'t, T> Lockable<'t, T> for MutexPreempt<T> where T: 't + ?Sized {
    type Guard = MutexPreemptGuard<'t, T>;
    type GuardMut = Self::Guard;

    fn lock(&'t self) -> Self::Guard { self.lock() }
    fn try_lock(&'t self) -> Option<Self::Guard> { self.try_lock() }
    fn lock_mut(&'t self) -> Self::GuardMut { self.lock() }
    fn try_lock_mut(&'t self) -> Option<Self::GuardMut> { self.try_lock() }
    fn is_locked(&self) -> bool { self.is_locked() }
    fn get_mut(&'t mut self) -> &mut T { self.get_mut() }
}
/// Implement `LockableSized` for [`MutexPreempt`].
impl<'t, T> LockableSized<'t, T> for MutexPreempt<T> where T: 't + Sized {
    fn into_inner(self) -> T { self.into_inner() }
}
