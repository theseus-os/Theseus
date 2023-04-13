use crate::Flavour;
use core::ops::{Deref, DerefMut};

pub struct RwLock<T, F>
where
    F: Flavour,
{
    inner: spin::RwLock<T>,
    data: F::LockData,
}

impl<T, F> RwLock<T, F>
where
    F: Flavour,
{
    #[inline]
    pub const fn new(value: T) -> Self {
        Self {
            inner: spin::RwLock::new(value),
            data: F::INIT,
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
            inner,
            data: &self.data,
            _guard: guard,
        })
    }

    #[inline]
    pub fn try_write(&self) -> Option<RwLockWriteGuard<'_, T, F>> {
        F::try_write_rw_lock(&self.inner, &self.data).map(|(inner, guard)| RwLockWriteGuard {
            inner,
            data: &self.data,
            _guard: guard,
        })
    }

    #[inline]
    pub fn read(&self) -> RwLockReadGuard<'_, T, F> {
        todo!();
    }

    pub fn write(&self) -> RwLockWriteGuard<'_, T, F> {
        todo!();
    }
}

pub struct RwLockReadGuard<'a, T, F>
where
    F: Flavour,
{
    inner: spin::RwLockReadGuard<'a, T>,
    data: &'a F::LockData,
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

pub struct RwLockWriteGuard<'a, T, F>
where
    F: Flavour,
{
    inner: spin::RwLockWriteGuard<'a, T>,
    data: &'a F::LockData,
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

pub mod spin {
    use core::{
        cell::UnsafeCell,
        ops::{Deref, DerefMut},
        sync::atomic::AtomicUsize,
    };

    const READER: usize = 1 << 1;
    const WRITER: usize = 1;

    pub struct RwLock<T> {
        lock: AtomicUsize,
        data: UnsafeCell<T>,
    }

    unsafe impl<T> Send for RwLock<T> where T: Send {}
    unsafe impl<T> Sync for RwLock<T> where T: Send + Sync {}

    // FIXME: impls for guards

    impl<T> RwLock<T> {
        #[inline]
        pub const fn new(value: T) -> Self {
            Self {
                lock: AtomicUsize::new(0),
                data: UnsafeCell::new(value),
            }
        }

        #[inline]
        pub fn try_read(&self) -> Option<RwLockReadGuard<T>> {
            todo!();
        }

        #[inline]
        pub fn try_read_weak(&self) -> Option<RwLockReadGuard<T>> {
            todo!();
        }

        #[inline]
        pub fn try_write(&self) -> Option<RwLockWriteGuard<T>> {
            todo!();
        }

        #[inline]
        pub fn try_write_weak(&self) -> Option<RwLockWriteGuard<T>> {
            todo!();
        }
    }

    pub struct RwLockReadGuard<'a, T>
    where
        T: 'a,
    {
        lock: &'a AtomicUsize,
        data: *const T,
    }

    impl<'a, T> Deref for RwLockReadGuard<'a, T>
    where
        T: 'a,
    {
        type Target = T;

        fn deref(&self) -> &Self::Target {
            unsafe { &*self.data }
        }
    }

    pub struct RwLockWriteGuard<'a, T>
    where
        T: 'a,
    {
        inner: &'a RwLock<T>,
        data: *mut T,
    }

    impl<'a, T> Deref for RwLockWriteGuard<'a, T>
    where
        T: 'a,
    {
        type Target = T;

        fn deref(&self) -> &Self::Target {
            unsafe { &*self.data }
        }
    }

    impl<'a, T> DerefMut for RwLockWriteGuard<'a, T>
    where
        T: 'a,
    {
        fn deref_mut(&mut self) -> &mut Self::Target {
            unsafe { &mut *self.data }
        }
    }
}
