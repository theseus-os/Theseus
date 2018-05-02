#![no_std]
#![cfg_attr(all(feature = "alloc", not(feature = "std")), feature(alloc))]
#![cfg_attr(all(feature = "collections", not(feature = "std")), feature(collections))]

//! A library that provides a way to logically own objects, whether or not
//! heap allocation is available.

#[cfg(feature = "std")]
extern crate std;
#[cfg(all(feature = "alloc", not(feature = "std")))]
extern crate alloc;
#[cfg(all(feature = "collections", not(feature = "std")))]
extern crate collections;

mod object;
mod slice;

pub use object::Managed;
pub use slice::ManagedSlice;
