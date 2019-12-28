//! This crate defines a trait of `Compositor`  .
//! A compositor composites a list of buffers to a single buffer.
//! The coordinate of a frame buffer represents the location of its origin (top-left point).

#![no_std]

extern crate frame_buffer;
extern crate shapes;
extern crate color;

use core::iter::IntoIterator;

use frame_buffer::{FrameBuffer, Pixel};
use shapes::{Coord, Rectangle};
use color::Color;

/// The compositor trait.
/// A compositor composites a list of buffers to a single buffer. It caches the information of incoming buffers for better performance.
/// The incoming list contains framebuffers and an iterator on bounding boxes. The compositor will refresh the part of every framebuffer in the bounding boxes.
/// `T` specifies the type of a bounding box. It implements `Mixable` which can mix the part of a framebuffer within the box to the final one.
pub trait Compositor<T: Mixable> {
    /// Composites the buffers in the bufferlist.
    ///
    /// # Arguments
    /// * `bufferlist`: an iterator over the buffers to be composited. Every item is a framebuffer and its position relative to the top-left of the screen. 
    /// * `final_fb`: the final framebuffer that the compositor will composite the bufferlist with.
    /// * `bounding_boxes`: a interator over the bounding boxes to be updated. The compositor will update the parts of every framebuffer in the boxes or the whole framebuffer if the argument is `None`.
    /// A compositor can cache the updated areas for better performance.
    fn composite<'a, U: IntoIterator<Item = T> + Clone, P: 'a + Pixel + From<Color>>(
        &mut self,
        bufferlist: impl IntoIterator<Item = FrameBufferUpdates<'a, P>>,
        final_fb: &mut FrameBuffer<P>,
        bounding_boxes: U
    ) -> Result<(), &'static str>;
}


/// The framebuffers to be composited together with the positions.
pub struct FrameBufferUpdates<'a, P: Pixel> {
    /// The framebuffer to be composited.
    pub framebuffer: &'a FrameBuffer<P>,
    /// The coordinate of the framebuffer where it is rendered to the final framebuffer.
    pub coordinate: Coord,
}

/// A `Mixable` is an item that can be mixed with the final framebuffer. A compositor can mix the parts in some bounding boxes to the final framebuffer rather than mix the whole framebuffer for better performance.
pub trait Mixable {
    /// Mix the item in the `src_fb` framebuffer with the final framebuffer. `src_coord` is the position of the source framebuffer relative to the top-left of the final buffer.
    fn mix_buffers<P: Pixel>(
        &self, 
        src_fb: &FrameBuffer<P>, 
        final_fb: &mut FrameBuffer<P>, 
        src_coord: Coord,        
    ) -> Result<(), &'static str>;
}

impl Mixable for Coord {
    fn mix_buffers<P: Pixel>(
        &self, 
        src_fb: &FrameBuffer<P>,
        final_fb: &mut FrameBuffer<P>, 
        src_coord: Coord,        
    ) -> Result<(), &'static str>{
        let relative_coord = self.clone() - src_coord;
        if src_fb.contains(relative_coord) {
            let pixel = src_fb.get_pixel(relative_coord)?;
            final_fb.draw_pixel(self.clone(), pixel);
        }
        Ok(())
    }
}

impl Mixable for Rectangle {
    fn mix_buffers<P: Pixel>(
        &self, 
        src_fb: &FrameBuffer<P>, 
        final_fb: &mut FrameBuffer<P>,
        src_coord: Coord,
    ) -> Result<(), &'static str> {
        let (final_width, final_height) = final_fb.get_size();
        let (src_width, src_height) = src_fb.get_size();
 
        // skip if the updated area is not in the final framebuffer
        let final_start = Coord::new(
            core::cmp::max(0, self.top_left.x),
            core::cmp::max(0, self.top_left.y)
        );

        let final_end = Coord::new(
            core::cmp::min(final_width as isize, self.bottom_right.x),
            core::cmp::min(final_height as isize, self.bottom_right.y)
        );
        if final_end.x < 0
            || final_start.x > final_width as isize
            || final_end.y < 0
            || final_start.y > final_height as isize
        {
            return Ok(());
        }
                
        // skip if the updated area is not in the source framebuffer
        let coordinate_start = final_start - src_coord;
        let coordinate_end = final_end - src_coord;
        if coordinate_end.x < 0
            || coordinate_start.x > src_width as isize
            || coordinate_end.y < 0
            || coordinate_start.y > src_height as isize
        {
            return Ok(());
        }

        let src_x_start = core::cmp::max(0, coordinate_start.x) as usize;
        let src_y_start = core::cmp::max(0, coordinate_start.y) as usize;

        // just draw the part within the final buffer
        let width = core::cmp::min(coordinate_end.x as usize, src_width) - src_x_start;
        let height = core::cmp::min(coordinate_end.y as usize, src_height) - src_y_start;

        // copy every line of the block to the final framebuffer.
        let src_buffer = &src_fb.buffer();
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
