//! A simple wrapper that prevents the inner object from being dropped.

#![no_std]

use core::{
    // fmt,
    mem::ManuallyDrop,
    ops::{Deref, DerefMut}
};

/// A wrapper for an inner object that ensures the inner object is never dropped.
///
/// This is effectively a safe version of `ManuallyDrop` with a restricted interface.
/// 
/// Auto-derefs to the inner object type `T`.
///
/// To re-take ownership of the object, call [`Self::into_inner()`].
#[derive(Debug)]
#[repr(transparent)]
pub struct NoDrop<T>(ManuallyDrop<T>);

impl<T> NoDrop<T> {
    /// Wraps the given `obj` in a `NoDrop` wrapper.
    pub const fn new(obj: T) -> NoDrop<T> {
        NoDrop(ManuallyDrop::new(obj))
    }

    /// Consumes this `NoDrop` wrapper and returns the inner object.
    pub const fn into_inner(self) -> T {
        ManuallyDrop::into_inner(self.0)
    }
}

impl<T> Deref for NoDrop<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl<T> DerefMut for NoDrop<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
