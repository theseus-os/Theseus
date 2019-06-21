//! This crate defines a trait of Compositor.
//! A compositor composites a list of buffers to a single buffer

#![no_std]

extern crate alloc;

use alloc::vec::Vec;

/// The compositor trait.
///* It composes a list of buffers to a single buffer
pub trait Compositor<T> {
    fn compose(bufferlist: Vec<(&T, i32, i32)>) -> Result<(), &'static str>;
}