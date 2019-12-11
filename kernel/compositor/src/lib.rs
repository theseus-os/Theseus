//! This crate defines a trait of `Compositor`  .
//! A compositor composites a list of buffers to a single buffer.
//! The coordinate of a frame buffer represents the location of its origin (top-left point).

#![no_std]

extern crate frame_buffer;

use core::iter::IntoIterator;
use frame_buffer::{Coord, FrameBuffer, Rectangle};

/// The compositor trait.
/// A compositor composites a list of buffers to a single buffer. It caches the information of incoming buffers for better performance.
/// The incoming list contains framebuffers and an iterator on shaped areas to be updated of every framebuffer. 
/// `T` specifies the type of a shape. It implements a `Mixable` which can mix a shaped area in the source framebuffer to the final one.
pub trait Compositor<T: Mixable> {
    /// Composites the buffers in the bufferlist.
    ///
    ///`bufferlist is a list of information about the buffers to be composited. An item in the list contains an interator on `Mixable` so that we can just update the areas specified by the mixers. The compositor will update the whole framebuffer if the interator of mixers is `None`. See the definition of `FrameBufferUpdates`.
    /// A compositor will cache the updated areas for better performance.
    fn composite<'a, U: IntoIterator<Item = T> + Clone>(
        &mut self,
        bufferlist: impl IntoIterator<Item = FrameBufferUpdates<'a, T, U>>,
        updates: U
    ) -> Result<(), &'static str>;
}


/// The framebuffers to be composited together with the information of their updated areas.
/// `T` specifies the shape of area to update and `U` is an iterator on `T`. `T` can be any shape that implements the `Mixable` trait such as a rectangle block or a point coordinate.
/// If the updates field is `None`, the compositor will update the whole framebuffer.
pub struct FrameBufferUpdates<'a, T: Mixable, U: IntoIterator<Item = T>> {
    /// The framebuffer to be composited.
    pub framebuffer: &'a dyn FrameBuffer,
    /// The coordinate of the framebuffer where it is rendered to the final framebuffer.
    pub coordinate: Coord,
    /// The updated blocks of the framebuffer. If `blocks` is `None`, the compositor would handle all the blocks of the framebuffer.
    pub updates: Option<U>,
}

/// A mixer is an item that can be mixed with the final framebuffer. A compositor can mix a list of shaped items with the final framebuffer rather than mix the whole framebuffer for better performance.
pub trait Mixable {
    /// Mix the item in the `src_fb` framebuffer with the final framebuffer. `src_coord` is the position of the source framebuffer relative to the top-left of the final buffer.
    fn mix_buffers(
        &self, 
        src_fb: &dyn FrameBuffer, 
        final_fb: &mut dyn FrameBuffer, 
        src_coord: Coord,        
    ) -> Result<(), &'static str>;
}

impl Mixable for Coord {
    fn mix_buffers(
        &self, 
        src_fb: &dyn FrameBuffer,
        final_fb: &mut dyn FrameBuffer, 
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

impl Mixable for Rectangle {
    fn mix_buffers(
        &self, 
        src_fb: &dyn FrameBuffer, 
        final_fb: &mut dyn FrameBuffer,
        src_coord: Coord,
    ) -> Result<(), &'static str> {
        let (final_width, final_height) = final_fb.get_size();
        let (src_width, src_height) = src_fb.get_size();
        // let src_buffer_len = src_width * src_height;
        // let (width, height) = self.get_size();
        // let block_pixels = height * src_width;

        // // The coordinate of the block relative to the screen
        // let start_index = self.top_left.y * src_height;
        // let coordinate_start = src_coord + (0, (CACHE_BLOCK_HEIGHT * self.index) as isize);
        // let end_index = start_index + block_pixels;
        

        // let coordinate_end = if end_index <= src_buffer_len {
        //     coordinate_start + (src_width as isize, CACHE_BLOCK_HEIGHT as isize)
        // } else {
        //     src_coord + (src_width as isize, src_height as isize)
        // };
        let final_start = Coord::new(
            core::cmp::max(0, self.top_left.x),
            core::cmp::max(0, self.top_left.y)
        );

        let final_end = Coord::new(
            core::cmp::min(final_width as isize, self.bottom_right.x),
            core::cmp::min(final_height as isize, self.bottom_right.y)
        );
        let coordinate_start = final_start - src_coord;
        let coordinate_end = final_end - src_coord;

        let src_buffer = &src_fb.buffer();

        // skip if the block is not in the screen
        if coordinate_end.x < 0
            || coordinate_start.x > final_width as isize
            || coordinate_end.y < 0
            || coordinate_start.y > final_height as isize
        {
            return Ok(());
        }

        let src_x_start = core::cmp::max(0, coordinate_start.x) as usize;
        let src_y_start = core::cmp::max(0, coordinate_start.y) as usize;

        // just draw the part which is within the final buffer
        let width = core::cmp::min(coordinate_end.x as usize, src_width) - src_x_start;
        let height = core::cmp::min(coordinate_end.y as usize, src_height) - src_y_start;

        // copy every line of the block to the final framebuffer.
        // let src_buffer = src_fb.buffer();
        for i in 0..height {
            let src_start = Coord::new(src_x_start as isize, (src_y_start + i) as isize);
            let src_start_index = match src_fb.index(src_start) {
                Some(index) => index,
                None => {continue;}
            };
            let src_end_index = src_start_index + width;
            let dest_start = src_start + src_coord;
            let dest_start_index =  match final_fb.index(dest_start) {
                Some(index) => index,
                None => {continue;}
            };
            final_fb.composite_buffer(&(src_buffer[src_start_index..src_end_index]), dest_start_index as usize);
        }

        Ok(())
    }
}
