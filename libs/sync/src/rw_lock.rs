use crate::spin;
use core::{
    fmt,
    mem::ManuallyDrop,
    ops::{Deref, DerefMut},
};

pub trait RwLockFlavor {
    /// Initial state of the additional data.
    const INIT: Self::LockData;

    /// Additional data stored in the lock.
    type LockData;

    /// Additional guard stored in the synchronisation guards.
    type Guard;

    /// Attempts to acquire the given lock with shared access.
    fn try_read<'a, T>(
        rw_lock: &'a spin::RwLock<T>,
        data: &'a Self::LockData,
    ) -> Option<(spin::RwLockReadGuard<'a, T>, Self::Guard)>
    where
        T: ?Sized;

    /// Attempts to acquire the given lock with exclusive access.
    fn try_write<'a, T>(
        rw_lock: &'a spin::RwLock<T>,
        data: &'a Self::LockData,
    ) -> Option<(spin::RwLockWriteGuard<'a, T>, Self::Guard)>
    where
        T: ?Sized;

    /// Acquires the given lock with shared access.
    fn read<'a, T>(
        rw_lock: &'a spin::RwLock<T>,
        data: &'a Self::LockData,
    ) -> (spin::RwLockReadGuard<'a, T>, Self::Guard)
    where
        T: ?Sized;

    /// Acquires the given lock with exclusive access.
    fn write<'a, T>(
        rw_lock: &'a spin::RwLock<T>,
        data: &'a Self::LockData,
    ) -> (spin::RwLockWriteGuard<'a, T>, Self::Guard)
    where
        T: ?Sized;

    /// Performs any necessary actions after unlocking the lock.
    fn post_unlock(data: &Self::LockData, is_writer_or_last_reader: bool);
}

/// A reader-writer lock.
pub struct RwLock<T, F>
where
    T: ?Sized,
    F: RwLockFlavor,
{
    data: F::LockData,
    inner: spin::RwLock<T>,
}

impl<T, F> RwLock<T, F>
where
    F: RwLockFlavor,
{
    /// Creates a new reader-writer lock.
    #[inline]
    pub const fn new(value: T) -> Self {
        Self {
            data: F::INIT,
            inner: spin::RwLock::new(value),
        }
    }

    /// Consumes this lock, returning the underlying data.
    #[inline]
    pub fn into_inner(self) -> T {
        self.inner.into_inner()
    }
}

impl<T, F> RwLock<T, F>
where
    T: ?Sized,
    F: RwLockFlavor,
{
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

impl<T, F> fmt::Debug for RwLock<T, F>
where
    T: ?Sized + fmt::Debug,
    F: RwLockFlavor,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut d = f.debug_struct("RwLock");
        match self.try_read() {
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

impl<T, F> Default for RwLock<T, F>
where
    T: Default,
    F: RwLockFlavor,
{
    fn default() -> Self {
        Self::new(T::default())
    }
}

/// RAII structure used to release the shared read access of a lock when
/// dropped.
pub struct RwLockReadGuard<'a, T, F>
where
    T: ?Sized,
    F: RwLockFlavor,
{
    inner: ManuallyDrop<spin::RwLockReadGuard<'a, T>>,
    data: &'a F::LockData,
    _guard: F::Guard,
}

impl<'a, T, F> Deref for RwLockReadGuard<'a, T, F>
where
    T: ?Sized,
    F: RwLockFlavor,
{
    type Target = T;

    #[inline]
    fn deref(&self) -> &Self::Target {
        self.inner.deref()
    }
}

impl<'a, T, F> Drop for RwLockReadGuard<'a, T, F>
where
    T: ?Sized,
    F: RwLockFlavor,
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
    T: ?Sized,
    F: RwLockFlavor,
{
    inner: ManuallyDrop<spin::RwLockWriteGuard<'a, T>>,
    data: &'a F::LockData,
    _guard: F::Guard,
}

impl<'a, T, F> Deref for RwLockWriteGuard<'a, T, F>
where
    T: ?Sized,
    F: RwLockFlavor,
{
    type Target = T;

    #[inline]
    fn deref(&self) -> &Self::Target {
        self.inner.deref()
    }
}

impl<'a, T, F> DerefMut for RwLockWriteGuard<'a, T, F>
where
    T: ?Sized,
    F: RwLockFlavor,
{
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.inner.deref_mut()
    }
}

impl<'a, T, F> Drop for RwLockWriteGuard<'a, T, F>
where
    T: ?Sized,
    F: RwLockFlavor,
{
    fn drop(&mut self) {
        unsafe { ManuallyDrop::drop(&mut self.inner) };
        F::post_unlock(self.data, true);
    }
}
