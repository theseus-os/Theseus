//! An abstraction over an owned value or borrowed reference
//! based on traits and marker types instead of enums.

#![no_std]

use core::{
    borrow::Borrow,
    ops::Deref,
};

/// A trait for abstracting over an owned value or borrowed reference to a type `T`.
///
/// You cannot implement this trait; it can only be used with [`Owned`] or [`Borrowed`].
///
/// The [`Owned`] and [`Borrowed`] wrapper types implement the following traits:
/// * [`AsRef`].
/// * [`Deref`] where `Target = T`.
pub trait OwnedOrBorrowed<T>: private::Sealed {
    /// * `true` if the wrapper type contains an owned value, i.e., for [`Owned`].
    /// * `false` if the wrapper type contains a borrowed reference, i.e., for [`Borrowed`].
    const OWNED: bool;
    /// The inner type of the owned value or borrowed reference.
    type Inner: Borrow<T>;
    
    /// Consumes this wrapper type and returns the contained value or borrowed reference.
    fn into_inner(self) -> Self::Inner;

    /// Returns a reference to the inner value.
    fn as_inner(&self) -> &Self::Inner;
}

/// A wrapper that indicates the contained value is an owned value of type `T`.
///
/// Implements the [`OwnedOrBorrowed`] trait.
pub struct Owned<T>(pub T);

/// A wrapper that indicates the contained value is a borrowed reference
/// to a value of type `T`.
///
/// Implements the [`OwnedOrBorrowed`] trait.
pub struct Borrowed<'t, T>(pub &'t T);

impl<T> OwnedOrBorrowed<T> for Owned<T> {
    const OWNED: bool = true;
    type Inner = T;
    fn into_inner(self) -> Self::Inner { self.0 }
    fn as_inner(&self) -> &Self::Inner { &self.0 }
}

impl<'t, T> OwnedOrBorrowed<T> for Borrowed<'t, T> {
    const OWNED: bool = false;
    type Inner = &'t T;
    fn into_inner(self) -> Self::Inner { self.0 }
    fn as_inner(&self) -> &Self::Inner { &self.0 }
}

impl<T> AsRef<T> for Owned<T> {
    fn as_ref(&self) -> &T {
        self.as_inner().borrow()
    }
}
impl<T> Deref for Owned<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        self.as_ref()
    }
}

impl<'t, T> AsRef<T> for Borrowed<'t, T> {
    fn as_ref(&self) -> &T {
        self.as_inner().borrow()
    }
}
impl<'t, T> Deref for Borrowed<'t, T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        self.as_ref()
    }
}

// Ensure no foreign crates can implement the `OwnedBorrowed` trait.
impl<T>     private::Sealed for Owned<T> { }
impl<'t, T> private::Sealed for Borrowed<'t, T> { }

mod private {
    pub trait Sealed { }
}
