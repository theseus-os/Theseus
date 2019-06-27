//! This crate defines a trait of Compositor.
//! A compositor composites a list of buffers to a single buffer

#![no_std]

extern crate alloc;

use alloc::vec::Vec;

/// The compositor trait.
/// A compositor composes a list of buffers to a single buffer
pub trait Compositor<T> {
    /// compose the buffers in the bufferlist
    ///
    /// # Arguments
    ///
    /// * `bufferlist` - A list of buffers in the form of (buffer:T, x:i32, y:i32).
    /// For each item in the list, buffer is a buffer object to be composed. (x, y) specifies the location of the buffer to be composed in the final buffer. 
    fn compose(&mut self, bufferlist: Vec<(&T, i32, i32)>) -> Result<(), &'static str>;

    /// checks if a buffer at (x, y) is already updated
    fn cached(&self, buffer:&T, x:i32, y:i32) -> bool;
}