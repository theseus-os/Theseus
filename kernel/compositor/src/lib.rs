//! This crate defines a trait of `Compositor`  .
//! A compositor composites a list of buffers to a single buffer.
//! The coordinate of a frame buffer represents the location of its origin (top-left point).

#![no_std]

extern crate alloc;
extern crate frame_buffer;

use alloc::vec::{IntoIter};
use frame_buffer::{Coord};

/// The compositor trait.
/// A compositor composites a list of buffers to a single buffer.
pub trait Compositor<BufferInfo> {
    /// Composites the buffers in the bufferlist.
    ///
    /// # Arguments
    ///
    /// * `bufferlist` - A list of information of the buffers to be composited. The type for BufferInfo is generic so that we can implement various compositor with specific information
    fn composite(
        &mut self,
        bufferlist: IntoIter<BufferInfo>,
    ) -> Result<(), &'static str>;

    /// Checks if a buffer at coordinate is already cached since last updating.
    fn is_cached(&self, block: &[u32], coordinate: &Coord, width: usize) -> bool;
 
}
