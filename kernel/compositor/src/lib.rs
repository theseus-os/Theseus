//! This crate defines a trait of `Compositor`  .
//! A compositor composites a list of buffers to a single buffer.
//! The coordinate of a frame buffer represents the location of its origin (top-left point).

#![no_std]

extern crate alloc;
extern crate frame_buffer;

use alloc::vec::{Vec, IntoIter};
use frame_buffer::{FrameBuffer, Coord};

/// The compositor trait.
/// A compositor composites a list of buffers to a single buffer.
pub trait Compositor<Buffer> {
    /// Composites the buffers in the bufferlist.
    ///
    /// # Arguments
    ///
    /// * `bufferlist` - A list of buffers in the form of (buffer:T, coordinate: Coord).
    /// For each tuple in the list, `buffer` is a buffer object to be composited. `coordinate` specifies the buffer relative to the final buffer.
    fn composite(
        &mut self,
        mut bufferlist: IntoIter<Buffer>,
    ) -> Result<(), &'static str>;

    /// Checks if a buffer at coordinate is already cached since last updating.
    fn is_cached(&self, block: &[u32], coordinate: &Coord, width: usize) -> bool;
 
}
