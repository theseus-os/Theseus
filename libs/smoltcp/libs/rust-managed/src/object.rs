use core::ops::{Deref, DerefMut};
use core::fmt;

#[cfg(feature = "std")]
use std::boxed::Box;
#[cfg(all(feature = "alloc", not(feature = "std")))]
use alloc::boxed::Box;
#[cfg(feature = "std")]
use std::vec::Vec;
//#[cfg(all(feature = "alloc", feature = "collections", not(feature = "std")))]
#[cfg(feature = "alloc")]
use alloc::vec::Vec;
//use collections::vec::Vec;

/// A managed object.
///
/// This enum can be used to represent exclusive access to objects. In Rust, exclusive access
/// to an object is obtained by either owning the object, or owning a mutable pointer
/// to the object; hence, "managed".
///
/// The purpose of this enum is providing good ergonomics with `std` present while making
/// it possible to avoid having a heap at all (which of course means that `std` is not present).
/// To achieve this, the variants other than `Borrow` are only available when the corresponding
/// feature is opted in.
///
/// A function that requires a managed object should be generic over an `Into<Managed<'a, T>>`
/// argument; then, it will be possible to pass either a `Box<T>`, `Vec<T>`, or a `&'a mut T`
/// without any conversion at the call site.
///
/// Note that a `Vec<T>` converted into an `Into<Managed<'static, [T]>>` gets transformed
/// into a boxed slice, and can no longer be resized. See also
/// [ManagedSlice](enum.ManagedSlice.html), which does not have this drawback.
pub enum Managed<'a, T: 'a + ?Sized> {
    /// Borrowed variant.
    Borrowed(&'a mut T),
    /// Owned variant, only available with the `std` or `alloc` feature enabled.
    #[cfg(any(feature = "std", feature = "alloc"))]
    Owned(Box<T>)
}

impl<'a, T: 'a + ?Sized> fmt::Debug for Managed<'a, T>
        where T: fmt::Debug {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            &Managed::Borrowed(ref x) => write!(f, "Borrowed({:?})", x),
            #[cfg(any(feature = "std", feature = "alloc"))]
            &Managed::Owned(ref x)    => write!(f, "Owned({:?})", x)
        }

    }
}

impl<'a, T: 'a + ?Sized> From<&'a mut T> for Managed<'a, T> {
    fn from(value: &'a mut T) -> Self {
        Managed::Borrowed(value)
    }
}

#[cfg(any(feature = "std", feature = "alloc"))]
impl<T: ?Sized + 'static> From<Box<T>> for Managed<'static, T> {
    fn from(value: Box<T>) -> Self {
        Managed::Owned(value)
    }
}

//#[cfg(any(feature = "std", all(feature = "alloc", feature = "collections")))]
#[cfg(any(feature = "std", any(feature = "alloc", feature = "collections")))]
impl<T: 'static> From<Vec<T>> for Managed<'static, [T]> {
    fn from(value: Vec<T>) -> Self {
        Managed::Owned(value.into_boxed_slice())
    }
}

impl<'a, T: 'a + ?Sized> Deref for Managed<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        match self {
            &Managed::Borrowed(ref value) => value,
            #[cfg(any(feature = "std", feature = "alloc"))]
            &Managed::Owned(ref value) => value
        }
    }
}

impl<'a, T: 'a + ?Sized> DerefMut for Managed<'a, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        match self {
            &mut Managed::Borrowed(ref mut value) => value,
            #[cfg(any(feature = "std", feature = "alloc"))]
            &mut Managed::Owned(ref mut value) => value
        }
    }
}
