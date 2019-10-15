//! This crate defines a trait of `Compositor`  .
//! A compositor composites a list of buffers to a single buffer.
//! The coordinate of a frame buffer represents the location of its origin (top-left point).

#![no_std]

extern crate alloc;
extern crate frame_buffer;

use alloc::vec::{IntoIter};
use frame_buffer::{Coord};

/// The compositor trait.
/// A compositor composites a list of buffers to a single buffer. It caches the information of incoming buffers for better performance.
pub trait Compositor<T> {
    /// Composites the buffers in the bufferlist.
    ///
    /// # Arguments
    ///
    /// * `bufferlist` - A list of information about the buffers to be composited. The list is of generic type so that we can implement various compositor with different information
    fn composite(
        &mut self,
        bufferlist: IntoIter<T>,
    ) -> Result<(), &'static str>;

    /// Checks if a buffer block at `coordinate` is already cached since last updating.
    fn is_cached(&self, block: &[u32], coordinate: &Coord, width: usize) -> bool;
 
}
