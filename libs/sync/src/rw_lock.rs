use crate::{spin, RwLockFlavour};
use core::{
    mem::ManuallyDrop,
    ops::{Deref, DerefMut},
};

/// A reader-writer lock.
pub struct RwLock<T, F>
where
    F: RwLockFlavour,
{
    inner: spin::RwLock<T>,
    data: F::LockData,
}

impl<T, F> RwLock<T, F>
where
    F: RwLockFlavour,
{
    /// Creates a new reader-writer lock.
    #[inline]
    pub const fn new(value: T) -> Self {
        Self {
            inner: spin::RwLock::new(value),
            data: F::INIT,
        }
    }

    /// Consumes this lock, returning the underlying data.
    #[inline]
    pub fn into_inner(self) -> T {
        self.inner.into_inner()
    }

    /// Returns a mutable reference to the underlying data.
    #[inline]
    pub fn get_mut(&mut self) -> &mut T {
        self.inner.get_mut()
    }

    /// Returns the number of readers that currently hold the lock.
    #[inline]
    pub fn reader_count(&self) -> usize {
        self.inner.reader_count()
    }

    /// Returns the number of writers that currently hold the lock.
    #[inline]
    pub fn writer_count(&self) -> usize {
        self.inner.writer_count()
    }

    /// Attempts to acquire this lock with shared read access.
    ///
    /// This method may spuriously fail.
    #[inline]
    pub fn try_read(&self) -> Option<RwLockReadGuard<'_, T, F>> {
        F::try_read(&self.inner, &self.data).map(|(inner, guard)| RwLockReadGuard {
            inner: ManuallyDrop::new(inner),
            data: &self.data,
            _guard: guard,
        })
    }

    /// Attempts to acquire this lock with exclusive write access.
    ///
    /// This method may spuriously fail.
    #[inline]
    pub fn try_write(&self) -> Option<RwLockWriteGuard<'_, T, F>> {
        F::try_write(&self.inner, &self.data).map(|(inner, guard)| RwLockWriteGuard {
            inner: ManuallyDrop::new(inner),
            data: &self.data,
            _guard: guard,
        })
    }

    /// Locks theis lock with shared read access.
    #[inline]
    pub fn read(&self) -> RwLockReadGuard<'_, T, F> {
        let (inner, guard) = F::read(&self.inner, &self.data);
        RwLockReadGuard {
            inner: ManuallyDrop::new(inner),
            data: &self.data,
            _guard: guard,
        }
    }

    /// Locks this lock with exclusive write access.
    pub fn write(&self) -> RwLockWriteGuard<'_, T, F> {
        let (inner, guard) = F::write(&self.inner, &self.data);
        RwLockWriteGuard {
            inner: ManuallyDrop::new(inner),
            data: &self.data,
            _guard: guard,
        }
    }
}

/// RAII structure used to release the shared read access of a lock when
/// dropped.
pub struct RwLockReadGuard<'a, T, F>
where
    F: RwLockFlavour,
{
    inner: ManuallyDrop<spin::RwLockReadGuard<'a, T>>,
    data: &'a F::LockData,
    _guard: F::Guard,
}

impl<'a, T, F> Deref for RwLockReadGuard<'a, T, F>
where
    F: RwLockFlavour,
{
    type Target = T;

    #[inline]
    fn deref(&self) -> &Self::Target {
        self.inner.deref()
    }
}

impl<'a, T, F> Drop for RwLockReadGuard<'a, T, F>
where
    F: RwLockFlavour,
{
    fn drop(&mut self) {
        let reader_count = unsafe { ManuallyDrop::take(&mut self.inner) }.release();
        F::post_unlock(self.data, reader_count == 0);
    }
}

/// RAII structure used to release the exclusive write access of a lock when
/// dropped.
pub struct RwLockWriteGuard<'a, T, F>
where
    F: RwLockFlavour,
{
    inner: ManuallyDrop<spin::RwLockWriteGuard<'a, T>>,
    data: &'a F::LockData,
    _guard: F::Guard,
}

impl<'a, T, F> Deref for RwLockWriteGuard<'a, T, F>
where
    F: RwLockFlavour,
{
    type Target = T;

    #[inline]
    fn deref(&self) -> &Self::Target {
        self.inner.deref()
    }
}

impl<'a, T, F> DerefMut for RwLockWriteGuard<'a, T, F>
where
    F: RwLockFlavour,
{
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.inner.deref_mut()
    }
}

impl<'a, T, F> Drop for RwLockWriteGuard<'a, T, F>
where
    F: RwLockFlavour,
{
    fn drop(&mut self) {
        unsafe { ManuallyDrop::drop(&mut self.inner) };
        F::post_unlock(self.data, true);
    }
}
