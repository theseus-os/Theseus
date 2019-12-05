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
/// `T` is the type of cache used in this framebuffer.
pub trait Compositor<T: Cache> {
    /// Composites the buffers in the bufferlist.
    ///
    /// # Arguments
    ///
    /// * `bufferlist` - A list of information about the buffers to be composited. The list is of generic type so that we can implement various compositor with different information. `U` specifices the type of item to update in compositing. It can be a rectangle block or a point.
    fn composite<U: Mixer<T>>(
        &mut self,
        bufferlist: IntoIter<FrameBufferUpdates<'_, U>>,
    ) -> Result<(), &'static str>;
}


/// The framebuffers to be composited together with the information of their updated blocks.
pub struct FrameBufferUpdates<'a, T> {
    /// The framebuffer to be composited.
    pub framebuffer: &'a dyn FrameBuffer,
    /// The coordinate of the framebuffer where it is rendered to the final framebuffer.
    pub coordinate: Coord,
    /// The updated blocks of the framebuffer. If `blocks` is `None`, the compositor would handle all the blocks of the framebuffer.
    pub updates: Option<IntoIter<T>>,
}

/// A mixer is an item that can be mixed with the final framebuffer. A compositor can mix a list of shaped items with the final framebuffer rather than mix the whole framebuffer for better performance.
pub trait Mixer<T: Cache> {
    /// Mix the item in the `src_fb` framebuffer with the final framebuffer. `src_coord` is the position of the source framebuffer relative to the top-left of the screen and `cache` is the cache of the compositor.
    fn mix_with_final(
        &self, 
        src_fb: &dyn FrameBuffer, 
        src_coord: Coord,        
        cache: &mut BTreeMap<Coord, T>
    ) -> Result<(), &'static str>;
}

/// This trait provides generic methods for caches in different compositors.
pub trait Cache {
    /// Checks if a cache overlaps with another one
    fn overlaps_with(&self, cache: &Self) -> bool;
}