//! Convenience types that immutably and mutably deref into an arbitrary type
//! reachable from their owned inner type.
//!
//! These types can be used as a purely-safe alternative to replace some of the
//! typical use cases for self-referential types.
//! They can also be used to limit access to and visibility of an inner type
//! by acting as wrappers that restrict callers to only accessing their deref target type.

#![no_std]
#![feature(const_mut_refs)]

use core::ops::{Deref, DerefMut};

/// A struct that holds an inner value and a function
/// that is used deref the `Inner` value into a `&Ref`.
/// 
/// As with [`Deref`], the dereffer function must not fail.
/// It typically just accesses an arbitrary field reachable from `Inner`.
/// 
/// This is also useful to prevent a caller from accessing all of `Inner`,
/// rather only giving them access to `Ref`.
pub struct DerefsTo<Inner, Ref>
where
    Ref: ?Sized,
{
    /// The inner object that is used as the starting point to access
    /// the type `Ref`, and is thus passed into the below `deref_func`
    /// in order to return an `&Ref` in this struct's `Deref` impl.
    inner: Inner,
    /// The function that is called within the `Deref` impl
    /// to actually access and return the `&Ref`.
    deref_func: fn(&Inner) -> &Ref,
}
impl<Inner, Ref> DerefsTo<Inner, Ref>
where
    Ref: ?Sized,
{
    pub const fn new(inner: Inner, deref_func: fn(&Inner) -> &Ref) -> Self {
        Self { inner, deref_func }
    }
}
impl<Inner, Ref> Deref for DerefsTo<Inner, Ref>
where
    Ref: ?Sized,
{
    type Target = Ref;
    fn deref(&self) -> &Self::Target {
        (self.deref_func)(&self.inner)
    }
}


/// Similar to [`DerefsTo`], but supports mutable dereferencing too.
/// 
/// Because Ruse doesn't offer a way to abstract over mutability,
/// i.e., accept both `&T` and `&mut T`, this struct must handle the
/// `Deref` and `DerefMut` cases separately with individual functions.
pub struct DerefsToMut<Inner, Ref>
where
    Ref: ?Sized,
{
    inner: DerefsTo<Inner, Ref>,
    deref_mut_func: fn(&mut Inner) -> &mut Ref,
}
impl<Inner, Ref> DerefsToMut<Inner, Ref>
where
    Ref: ?Sized,
{
    pub const fn new(
        inner: Inner,
        deref_func: fn(&Inner) -> &Ref,
        deref_mut_func: fn(&mut Inner) -> &mut Ref,
    ) -> Self {
        Self { inner: DerefsTo::new(inner, deref_func), deref_mut_func }
    }
}
impl<Inner, Ref> Deref for DerefsToMut<Inner, Ref>
where
    Ref: ?Sized,
{
    type Target = Ref;
    fn deref(&self) -> &Self::Target {
        self.inner.deref()
    }
}

impl<Inner, Ref> DerefMut for DerefsToMut<Inner, Ref>
where
    Ref: ?Sized,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        (self.deref_mut_func)(&mut self.inner.inner)
    }
}
