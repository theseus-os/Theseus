//! Traits for items that are "Lockable", e.g., `Mutex`es.

#![no_std]

extern crate spin;
extern crate sync_irq;

use core::ops::{Deref, DerefMut};

/// A trait representing types that can be locked, e.g., `Mutex`es.
///
/// It also can represent types like `RwLock` (read-write lock) 
/// that allow multiple concurrent readers but only one concurrent writer. 
///
/// Note: an optional design choice would be to remove the generic `T` parameter
/// and instead assign it as an associated type, e.g., `type Inner: 't`.
/// 
pub trait Lockable<'t, T: 't + ?Sized> {
    /// The immutable "guard" type returned by the [`Self::lock()`] function.
    type Guard: Deref<Target = T>;

    /// The mutable "guard" type returned by the [`Self::lock_mut()`] function.
    ///
    /// For locks like `RwLock` that differentiate between read-only and read-write locks,
    /// this should be set to the read-write guard type.
    /// For locks like `Mutex` that only have one locking function,
    /// this should be set to the same type as [`Self::Guard`].
    type GuardMut: DerefMut<Target = T>;

    /// Obtain the lock in a blocking fashion, 
    /// returning an immutable guard that dereferences into the inner data.
    fn lock(&'t self) -> Self::Guard;

    /// Attempt to obtain the lock in a non-blocking fashion,
    /// returning an immutable guard that dereferences into the inner data.
    ///
    /// If the lock is already locked, this returns `None`.
    fn try_lock(&'t self) -> Option<Self::Guard>;

    /// Obtain the lock in a blocking fashion,
    /// returning a mutable guard that dereferences into the inner data.
    fn lock_mut(&'t self) -> Self::GuardMut;

    /// Attempt to obtain the lock in a non-blocking fashion,
    /// returning a mutable guard that dereferences into the inner data.
    /// 
    /// If the lock is already locked, this returns `None`.
    fn try_lock_mut(&'t self) -> Option<Self::GuardMut>;

    /// Returns `true` if this lock is currently locked. 
    ///
    /// For types like `RwLock` that can lock in a read-only or read-write manner,
    /// this should return `true` if the singular writer lock is obtained,
    /// and `false` if only the reader lock is obtained.
    fn is_locked(&self) -> bool;

    /// Returns a mutable reference to the underlying data.
    fn get_mut(&'t mut self) -> &'t mut T;
}


/// An extension of the [`Lockable`] trait that adds the `into_inner()` method
/// only for types `T` that are `Sized`. 
pub trait LockableSized<'t, T: 't + Sized>: Lockable<'t, T> {
    /// Consumes the lock, returning the underlying data.
    fn into_inner(self) -> T;
}

/// Implement `Lockable` for [`spin::Mutex`].
impl<'t, T> Lockable<'t, T> for spin::Mutex<T> where T: 't + ?Sized {
    type Guard = spin::MutexGuard<'t, T>;
    type GuardMut = Self::Guard;

    fn lock(&'t self) -> Self::Guard { self.lock() }
    fn try_lock(&'t self) -> Option<Self::Guard> { self.try_lock() }
    fn lock_mut(&'t self) -> Self::GuardMut { self.lock() }
    fn try_lock_mut(&'t self) -> Option<Self::GuardMut> { self.try_lock() }
    fn is_locked(&self) -> bool { self.is_locked() }
    fn get_mut(&'t mut self) -> &mut T { self.get_mut() }
}
/// Implement `LockableSized` for [`spin::Mutex`].
impl<'t, T> LockableSized<'t, T> for spin::Mutex<T> where T: 't + Sized {
    fn into_inner(self) -> T { self.into_inner() }
}

/// Implement `Lockable` for [`spin::RwLock`].
impl<'t, T> Lockable<'t, T> for spin::RwLock<T> where T: 't + ?Sized {
    type Guard = spin::RwLockReadGuard<'t, T>;
    type GuardMut = spin::RwLockWriteGuard<'t, T>;

    fn lock(&'t self) -> Self::Guard { self.read() }
    fn try_lock(&'t self) -> Option<Self::Guard> { self.try_read() }
    fn lock_mut(&'t self) -> Self::GuardMut { self.write() }
    fn try_lock_mut(&'t self) -> Option<Self::GuardMut> { self.try_write() }
    fn is_locked(&self) -> bool { self.writer_count() > 0 }
    fn get_mut(&'t mut self) -> &mut T { self.get_mut() }
}
/// Implement `LockableSized` for [`spin::RwLock`].
impl<'t, T> LockableSized<'t, T> for spin::RwLock<T> where T: 't + Sized {
    fn into_inner(self) -> T { self.into_inner() }
}

/// Implement `Lockable` for [`sync_irq::IrqSafeMutex`].
impl<'t, T> Lockable<'t, T> for sync_irq::IrqSafeMutex<T> where T: 't {
    type Guard = sync_irq::IrqSafeMutexGuard<'t, T>;
    type GuardMut = Self::Guard;

    fn lock(&'t self) -> Self::Guard { self.lock() }
    fn try_lock(&'t self) -> Option<Self::Guard> { self.try_lock() }
    fn lock_mut(&'t self) -> Self::GuardMut { self.lock() }
    fn try_lock_mut(&'t self) -> Option<Self::GuardMut> { self.try_lock() }
    fn is_locked(&self) -> bool { self.is_locked() }
    fn get_mut(&'t mut self) -> &mut T { self.get_mut() }
}
/// Implement `LockableSized` for [`sync_irq::IrqSafeMutex`].
impl<'t, T> LockableSized<'t, T> for sync_irq::IrqSafeMutex<T> where T: 't + Sized {
    fn into_inner(self) -> T { self.into_inner() }
}

/// Implement `Lockable` for [`sync_irq::IrqSafeRwLock`].
impl<'t, T> Lockable<'t, T> for sync_irq::IrqSafeRwLock<T> where T: 't {
    type Guard = sync_irq::IrqSafeRwLockReadGuard<'t, T>;
    type GuardMut = sync_irq::IrqSafeRwLockWriteGuard<'t, T>;

    fn lock(&'t self) -> Self::Guard { self.read() }
    fn try_lock(&'t self) -> Option<Self::Guard> { self.try_read() }
    fn lock_mut(&'t self) -> Self::GuardMut { self.write() }
    fn try_lock_mut(&'t self) -> Option<Self::GuardMut> { self.try_write() }
    fn is_locked(&self) -> bool { self.writer_count() > 0 }
    fn get_mut(&'t mut self) -> &mut T { self.get_mut() }
}
/// Implement `LockableSized` for [`sync_irq::IrqSafeRwLock`].
impl<'t, T> LockableSized<'t, T> for sync_irq::IrqSafeRwLock<T> where T: 't + Sized {
    fn into_inner(self) -> T { self.into_inner() }
}
