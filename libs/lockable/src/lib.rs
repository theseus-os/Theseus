//! Traits for items that are "Lockable", e.g., `Mutex`es.

extern crate spin;
extern crate irq_safety;

/// A trait representing types that can be locked, e.g., `Mutex`es.
///
/// It also can represent types like `RwLock` (read-write lock) 
/// that allow multiple concurrent readers but only one concurrent writer. 
pub trait Lockable<'g, T: 'g> {
    /// The immutable "guard" type returned by the [`Self::lock()`] function.
    type Guard;

    /// The mutable "guard" type returned by the [`Self::lock_mut()`] function.
    ///
    /// For locks like `RwLock` that differentiate between read-only and read-write locks,
    /// this should be set to the read-write guard type.
    /// For locks like `Mutex` that only have one locking function,
    /// this should be set to the same type as [`Self::Guard`].
    type GuardMut;

    /// Obtain the lock in a blocking fashion, 
    /// returning an immutable guard that dereferences into the inner data.
    fn lock(&'g self) -> Self::Guard;

    /// Attempt to obtain the lock in a non-blocking fashion,
    /// returning an immutable guard that dereferences into the inner data.
    ///
    /// If the lock is already locked, this returns `None`.
    fn try_lock(&'g self) -> Option<Self::Guard>;

    /// Obtain the lock in a blocking fashion,
    /// returning a mutable guard that dereferences into the inner data.
    fn lock_mut(&'g self) -> Self::GuardMut;

    /// Attempt to obtain the lock in a non-blocking fashion,
    /// returning a mutable guard that dereferences into the inner data.
    /// 
    /// If the lock is already locked, this returns `None`.
    fn try_lock_mut(&'g self) -> Option<Self::GuardMut>;

    /// Returns `true` if this lock is currently locked. 
    ///
    /// For types like `RwLock` that can lock in a read-only or read-write manner,
    /// this should return `true` if the singular writer lock is obtained,
    /// and `false` if only the reader lock is obtained.
    fn is_locked(&'g self) -> bool;

    /// Returns a mutable reference to the underlying data.
    fn get_mut(&'g mut self) -> &'g mut T;

    /// Consumes the lock, returning the underlying data.
    fn into_inner(self) -> T;
}


/// Implement `Lockable` for [`spin::Mutex`].
impl<'g, T> Lockable<'g, T> for spin::Mutex<T> where T: 'g {
    type Guard = spin::MutexGuard<'g, T>;
    type GuardMut = Self::Guard;

    fn lock(&'g self) -> Self::Guard { self.lock() }
    fn try_lock(&'g self) -> Option<Self::Guard> { self.try_lock() }
    fn lock_mut(&'g self) -> Self::GuardMut { self.lock() }
    fn try_lock_mut(&'g self) -> Option<Self::GuardMut> { self.try_lock() }
    fn is_locked(&'g self) -> bool { self.is_locked() }
    fn get_mut(&'g mut self) -> &'g mut T { self.get_mut() }
    fn into_inner(self) -> T { self.into_inner() }
}

/// Implement `Lockable` for [`spin::RwLock`].
impl<'g, T> Lockable<'g, T> for spin::RwLock<T> where T: 'g {
    type Guard = spin::RwLockReadGuard<'g, T>;
    type GuardMut = spin::RwLockWriteGuard<'g, T>;

    fn lock(&'g self) -> Self::Guard { self.read() }
    fn try_lock(&'g self) -> Option<Self::Guard> { self.try_read() }
    fn lock_mut(&'g self) -> Self::GuardMut { self.write() }
    fn try_lock_mut(&'g self) -> Option<Self::GuardMut> { self.try_write() }
    fn is_locked(&'g self) -> bool { self.writer_count() > 0 }
    fn get_mut(&'g mut self) -> &'g mut T { self.get_mut() }
    fn into_inner(self) -> T { self.into_inner() }
}

/// Implement `Lockable` for [`irq_safety::MutexIrqSafe`].
impl<'g, T> Lockable<'g, T> for irq_safety::MutexIrqSafe<T> where T: 'g {
    type Guard = irq_safety::MutexIrqSafeGuard<'g, T>;
    type GuardMut = Self::Guard;

    fn lock(&'g self) -> Self::Guard { self.lock() }
    fn try_lock(&'g self) -> Option<Self::Guard> { self.try_lock() }
    fn lock_mut(&'g self) -> Self::GuardMut { self.lock() }
    fn try_lock_mut(&'g self) -> Option<Self::GuardMut> { self.try_lock() }
    fn is_locked(&'g self) -> bool { self.is_locked() }
    fn get_mut(&'g mut self) -> &'g mut T { self.get_mut() }
    fn into_inner(self) -> T { self.into_inner() }
}

/// Implement `Lockable` for [`irq_safety::RwLockIrqSafe`].
impl<'g, T> Lockable<'g, T> for irq_safety::RwLockIrqSafe<T> where T: 'g {
    type Guard = irq_safety::RwLockIrqSafeReadGuard<'g, T>;
    type GuardMut = irq_safety::RwLockIrqSafeWriteGuard<'g, T>;

    fn lock(&'g self) -> Self::Guard { self.read() }
    fn try_lock(&'g self) -> Option<Self::Guard> { self.try_read() }
    fn lock_mut(&'g self) -> Self::GuardMut { self.write() }
    fn try_lock_mut(&'g self) -> Option<Self::GuardMut> { self.try_write() }
    fn is_locked(&'g self) -> bool { self.writer_count() > 0 }
    fn get_mut(&'g mut self) -> &'g mut T { self.get_mut() }
    fn into_inner(self) -> T { self.into_inner() }
}
