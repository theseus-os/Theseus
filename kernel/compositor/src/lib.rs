//! This crate defines a trait of `Compositor`  .
//! A compositor composites a list of buffers to a single buffer.

#![no_std]

extern crate alloc;
extern crate frame_buffer;

use alloc::vec::Vec;
use frame_buffer::{FrameBuffer, ICoord};

/// The compositor trait.
/// A compositor composites a list of buffers to a single buffer.
pub trait Compositor {
    /// Composites the buffers in the bufferlist.
    ///
    /// # Arguments
    ///
    /// * `bufferlist` - A list of buffers in the form of (buffer:T, coordinate: ICoord).
    /// For each tuple in the list, `buffer` is a buffer object to be composited. `coordinate` specifies the relative location of the buffer in the final buffer.
    fn composite(
        &mut self,
        bufferlist: Vec<(&dyn FrameBuffer, ICoord)>,
    ) -> Result<(), &'static str>;

    /// Checks if a buffer at coordinate is already cached since last updating.
    fn cached(&self, buffer: &dyn FrameBuffer, coordinate: ICoord) -> bool;
}
