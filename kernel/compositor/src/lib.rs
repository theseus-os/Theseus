//! This crate defines a trait of `Compositor`  .
//! A compositor composites a list of buffers to a single buffer.
//! The coordinate of a frame buffer represents the location of its origin (top-left point).

#![no_std]

extern crate alloc;
extern crate frame_buffer;
extern crate hashbrown;

use core::hash::{Hash, Hasher, BuildHasher};
use core::iter::Iterator;
use hashbrown::hash_map::{DefaultHashBuilder};
use frame_buffer::{Coord, FrameBuffer};
use alloc::collections::BTreeMap;
use alloc::boxed::Box;
use alloc::vec::Vec;

/// The compositor trait.
/// A compositor composites a list of buffers to a single buffer. It caches the information of incoming buffers for better performance.
/// `T` is the type of cache used in this framebuffer.
pub trait Compositor<'a, T: 'a + Mixer> {
    /// Composites the buffers in the bufferlist.
    ///
    /// # Arguments
    ///
    /// * `bufferlist` - A list of information about the buffers to be composited. The list is of generic type so that we can implement various compositor with different information. `U` specifices the type of item to update in compositing. It can be a rectangle block or a point.
    fn composite(
        &mut self,
        mut bufferlist: impl Iterator<Item = FrameBufferUpdates<'a, T>>,
    ) -> Result<(), &'static str>;
}


/// The framebuffers to be composited together with the information of their updated blocks.
pub struct FrameBufferUpdates<'a, T: Mixer> {
    /// The framebuffer to be composited.
    pub framebuffer: &'a dyn FrameBuffer,
    /// The coordinate of the framebuffer where it is rendered to the final framebuffer.
    pub coordinate: Coord,
    /// The updated blocks of the framebuffer. If `blocks` is `None`, the compositor would handle all the blocks of the framebuffer.
    pub updates: Option<&'a [T]>,
}

/// A mixer is an item that can be mixed with the final framebuffer. A compositor can mix a list of shaped items with the final framebuffer rather than mix the whole framebuffer for better performance.
pub trait Mixer {
    /// Mix the item in the `src_fb` framebuffer with the final framebuffer. `src_coord` is the position of the source framebuffer relative to the top-left of the screen and `cache` is the cache of the compositor.
    fn mix_buffers(
        &self, 
        src_fb: &dyn FrameBuffer, 
        final_fb: &mut Box<dyn FrameBuffer + Send>, 
        src_coord: Coord,        
    ) -> Result<(), &'static str>;
}

impl Mixer for Coord {
    fn mix_buffers(
        &self, 
        src_fb: &dyn FrameBuffer,
        final_fb: &mut Box<dyn FrameBuffer + Send>, 
        src_coord: Coord,        
    ) -> Result<(), &'static str>{
        let relative_coord = self.clone() - src_coord;
        if src_fb.contains(relative_coord) {
            let pixel = src_fb.get_pixel(relative_coord)?;
            final_fb.draw_pixel(self.clone(), pixel);
        }

        // remove the cache containing the pixel
        // let keys: Vec<_> = caches.keys().cloned().collect();
        // for key in keys {
        //     if let Some(cache) = caches.get_mut(&key) {
        //         if cache.contains(self.clone()) {
        //             caches.remove(&key);
        //             break;
        //         }
        //     };
        // }

        Ok(())
    }
}