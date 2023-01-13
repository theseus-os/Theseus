use core::ops::{Deref, DerefMut};

use crate::prevention::{GuardData, LockData, MutexFlavour};

pub struct Mutex<P, T>
where
    P: MutexFlavour,
{
    inner: spin::Mutex<T>,
    prevention: P::LockData,
}

impl<P, T> Mutex<P, T>
where
    P: MutexFlavour,
{
    #[inline]
    pub fn new(data: T) -> Self {
        Self {
            inner: spin::Mutex::new(data),
            prevention: P::LockData::new(),
        }
    }

    #[inline]
    pub fn try_lock(&self) -> Option<MutexGuard<'_, P, T>> {
        if self.inner.is_locked() {
            None
        } else {
            let prevention = P::GuardData::new(&self.prevention);
            self.inner.try_lock().map(|guard| MutexGuard {
                inner: guard,
                _prevention: prevention,
            })
        }
    }

    #[inline]
    pub fn lock(&self) -> MutexGuard<'_, P, T> {
        if let Some(guard) = self.try_lock() {
            return guard;
        }

        P::slow_path(&self.prevention, || self.try_lock())
    }
}

pub struct MutexGuard<'a, P, T>
where
    P: MutexFlavour,
{
    inner: spin::MutexGuard<'a, T>,
    // The guard data is dropped after the inner mutex guard.
    _prevention: P::GuardData<'a>,
}

impl<P, T> Deref for MutexGuard<'_, P, T>
where
    P: MutexFlavour,
{
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.inner.deref()
    }
}

impl<P, T> DerefMut for MutexGuard<'_, P, T>
where
    P: MutexFlavour,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.inner.deref_mut()
    }
}
