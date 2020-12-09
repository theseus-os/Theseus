//! Use debug printlns, without the trait bounds (using specialization to
//! find the right impl anyway).

#![no_std]
#![allow(incomplete_features)]
#![feature(specialization)]

#[cfg(test)]
#[macro_use] extern crate std;

use core::fmt;


/// Formats the given argument using its `Debug` trait definition
/// and returns the `core::fmt::Arguments` containing its Debug output,
/// iff the argument's type implements the Debug trait. 
/// 
/// If it does *not* implement the Debug trait, then the type's name is printed instead.
///
/// # Examples
/// ```
/// #[macro_use] extern crate debugit;
///
/// println!("{}", debugit!(my_struct));
/// ```
#[macro_export]
macro_rules! debugit {
    ($value:expr) => {
        format_args!("{:?}", $crate::DebugIt(&$value))
    }
}

/// A helper type for using with the `debugit!()` macro.
#[derive(Copy, Clone)]
pub struct DebugIt<T>(pub T);

impl<T> fmt::Debug for DebugIt<T> {
    default fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{{ non-Debug: {} }}", core::any::type_name::<T>())
    }
}

impl<T> fmt::Debug for DebugIt<T>
    where T: fmt::Debug
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.0.fmt(f)
    }
}
