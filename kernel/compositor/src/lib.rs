//! This crate defines a trait of `Compositor`  .
//! A compositor composites a list of buffers to a single buffer.
//! The coordinate of a frame buffer represents the location of its origin (top-left point).

#![no_std]

extern crate alloc;
extern crate frame_buffer;

use alloc::vec::{IntoIter};
use frame_buffer::{Coord, FrameBuffer};

/// The compositor trait.
/// A compositor composites a list of buffers to a single buffer. It caches the information of incoming buffers for better performance.
pub trait Compositor<'a, T> {
    /// Composites the buffers in the bufferlist.
    ///
    /// # Arguments
    ///
    /// * `bufferlist` - A list of information about the buffers to be composited. The list is of generic type so that we can implement various compositor with different information
    fn composite(
        &mut self,
        bufferlist: IntoIter<FrameBufferBlocks<'a, T>>,
    ) -> Result<(), &'static str>;

    /// Composites the pixels in these bufferlist
    /// # Arguments
    ///
    /// * `bufferlist` - A list of information about the buffers to be composited. The list is of generic type so that we can implement various compositor with different information
    /// * `abs_coords` - A list of coordinates relative to the origin (top-left) of the screen. The compositor will get the relative position of these pixels in every framebuffer and composites them.
    fn composite_pixels(&mut self, bufferlist: IntoIter<FrameBufferBlocks<'a, Coord>>) -> Result<(), &'static str>; 
}


/// The framebuffers to be composited together with the information of their updated blocks.
pub struct FrameBufferBlocks<'a, T> {
    /// The framebuffer to be composited.
    pub framebuffer: &'a dyn FrameBuffer,
    /// The coordinate of the framebuffer where it is rendered to the final framebuffer.
    pub coordinate: Coord,
    /// The updated blocks of the framebuffer. If `blocks` is `None`, the compositor would handle all the blocks of the framebuffer.
    pub blocks: Option<IntoIter<T>>,
}
