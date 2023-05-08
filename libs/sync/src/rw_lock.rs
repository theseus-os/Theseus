use crate::{spin, Flavour};
use core::{
    mem::ManuallyDrop,
    ops::{Deref, DerefMut},
};

pub struct RwLock<T, F>
where
    F: Flavour,
{
    inner: spin::RwLock<T>,
    data: F::RwLockData,
}

impl<T, F> RwLock<T, F>
where
    F: Flavour,
{
    #[inline]
    pub const fn new(value: T) -> Self {
        Self {
            inner: spin::RwLock::new(value),
            data: F::RW_LOCK_INIT,
        }
    }

    #[inline]
    pub fn into_inner(self) -> T {
        self.inner.into_inner()
    }

    #[inline]
    pub fn get_mut(&mut self) -> &mut T {
        self.inner.get_mut()
    }

    #[inline]
    pub fn reader_count(&self) -> usize {
        self.inner.reader_count()
    }

    #[inline]
    pub fn writer_count(&self) -> usize {
        self.inner.writer_count()
    }

    #[inline]
    pub fn try_read(&self) -> Option<RwLockReadGuard<'_, T, F>> {
        F::try_read_rw_lock(&self.inner, &self.data).map(|(inner, guard)| RwLockReadGuard {
            inner: ManuallyDrop::new(inner),
            data: &self.data,
            _guard: guard,
        })
    }

    #[inline]
    pub fn try_write(&self) -> Option<RwLockWriteGuard<'_, T, F>> {
        F::try_write_rw_lock(&self.inner, &self.data).map(|(inner, guard)| RwLockWriteGuard {
            inner: ManuallyDrop::new(inner),
            data: &self.data,
            _guard: guard,
        })
    }

    #[inline]
    pub fn read(&self) -> RwLockReadGuard<'_, T, F> {
        let (inner, guard) = F::read_rw_lock(&self.inner, &self.data);
        RwLockReadGuard {
            inner: ManuallyDrop::new(inner),
            data: &self.data,
            _guard: guard,
        }
    }

    pub fn write(&self) -> RwLockWriteGuard<'_, T, F> {
        let (inner, guard) = F::write_rw_lock(&self.inner, &self.data);
        RwLockWriteGuard {
            inner: ManuallyDrop::new(inner),
            data: &self.data,
            _guard: guard,
        }
    }
}

pub struct RwLockReadGuard<'a, T, F>
where
    F: Flavour,
{
    inner: ManuallyDrop<spin::RwLockReadGuard<'a, T>>,
    data: &'a F::RwLockData,
    _guard: F::Guard,
}

impl<'a, T, F> Deref for RwLockReadGuard<'a, T, F>
where
    F: Flavour,
{
    type Target = T;

    #[inline]
    fn deref(&self) -> &Self::Target {
        self.inner.deref()
    }
}

impl<'a, T, F> Drop for RwLockReadGuard<'a, T, F>
where
    F: Flavour,
{
    fn drop(&mut self) {
        let reader_count = unsafe { ManuallyDrop::take(&mut self.inner) }.release();
        F::post_rw_lock_unlock(self.data, reader_count == 0);
    }
}

pub struct RwLockWriteGuard<'a, T, F>
where
    F: Flavour,
{
    inner: ManuallyDrop<spin::RwLockWriteGuard<'a, T>>,
    data: &'a F::RwLockData,
    _guard: F::Guard,
}

impl<'a, T, F> Deref for RwLockWriteGuard<'a, T, F>
where
    F: Flavour,
{
    type Target = T;

    #[inline]
    fn deref(&self) -> &Self::Target {
        self.inner.deref()
    }
}

impl<'a, T, F> DerefMut for RwLockWriteGuard<'a, T, F>
where
    F: Flavour,
{
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.inner.deref_mut()
    }
}

impl<'a, T, F> Drop for RwLockWriteGuard<'a, T, F>
where
    F: Flavour,
{
    fn drop(&mut self) {
        unsafe { ManuallyDrop::drop(&mut self.inner) };
        F::post_rw_lock_unlock(self.data, true);
    }
}
