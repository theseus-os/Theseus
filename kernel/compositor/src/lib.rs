//! This crate defines a trait of `Compositor`  .
//! A compositor composites a list of buffers to a single buffer.
//! The coordinate of a frame buffer represents the location of its origin (top-left point).

#![no_std]

extern crate alloc;
extern crate frame_buffer;

use alloc::vec::{IntoIter};
use frame_buffer::{Coord, FrameBuffer};
use alloc::collections::BTreeMap;

/// The compositor trait.
/// A compositor composites a list of buffers to a single buffer. It caches the information of incoming buffers for better performance.
pub trait Compositor<'a, T: Cache> {
    /// Composites the buffers in the bufferlist.
    ///
    /// # Arguments
    ///
    /// * `bufferlist` - A list of information about the buffers to be composited. The list is of generic type so that we can implement various compositor with different information
    fn composite<U: Mixer<T>>(
        &mut self,
        bufferlist: IntoIter<FrameBufferBlocks<'a, U>>,
    ) -> Result<(), &'static str>;
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

pub trait Mixer<T: Cache> {
    fn mix_with_final(
        &self, 
        src_fb: &dyn FrameBuffer, 
        src_coord: Coord,        
        cache: &mut BTreeMap<Coord, T>
    ) -> Result<(), &'static str>;
}

pub trait Cache {
    fn overlaps_with(&self, cache: &Self) -> bool;
}