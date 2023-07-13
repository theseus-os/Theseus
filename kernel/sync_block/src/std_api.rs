//! Defines a mutex and read-write lock with fallible locking methods.
//!
//! Primarily used to simplify porting applications to Theseus as the types have
//! a similar API to the standard library.

pub use crate::{MutexGuard, RwLockReadGuard, RwLockWriteGuard};

#[derive(Debug, Default)]
pub struct Mutex<T> {
    inner: crate::Mutex<T>,
}

impl<T> Mutex<T> {
    pub const fn new(value: T) -> Self {
        Self {
            inner: crate::Mutex::new(value),
        }
    }

    #[allow(clippy::result_unit_err)]
    pub fn lock(&self) -> Result<MutexGuard<T>, ()> {
        Ok(self.inner.lock())
    }
}

#[derive(Debug, Default)]
pub struct RwLock<T> {
    inner: crate::RwLock<T>,
}

impl<T> RwLock<T> {
    pub const fn new(value: T) -> Self {
        Self {
            inner: crate::RwLock::new(value),
        }
    }

    #[allow(clippy::result_unit_err)]
    pub fn read(&self) -> Result<RwLockReadGuard<T>, ()> {
        Ok(self.inner.read())
    }

    #[allow(clippy::result_unit_err)]
    pub fn write(&self) -> Result<RwLockWriteGuard<T>, ()> {
        Ok(self.inner.write())
    }
}
