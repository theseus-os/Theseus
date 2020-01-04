//! This crate defines a trait of `Compositor`  .
//! A compositor composites a list of buffers to a single buffer.
//! The coordinate of a framebuffer represents the location of its origin (top-left point).

#![no_std]

extern crate frame_buffer;
extern crate shapes;

use core::iter::IntoIterator;

use frame_buffer::{FrameBuffer, Pixel};
use shapes::{Coord, Rectangle};

/// A compositor composites (combines or blends) a series of "source" framebuffers onto a single "destination" framebuffer. 
/// The type parameter `R` allows a compositor to support multiple types of regions or "bounding boxes", 
/// given by the trait bound `BlendableRegion`.
pub trait Compositor<R: BlendableRegion> {
    /// Composites the framebuffers in the list of source framebuffers `src_fbs` into the destination framebuffer `dest_fb`.
    ///
    /// # Arguments
    /// * `src_fbs`: an iterator over the source framebuffers to be composited and where in the `dest_fb` they should be composited. 
    /// * `dest_fb`: the destination framebuffer that will contain the result of the composited source framebuffers.
    /// * `bounding_boxes`: an iterator over bounding boxes that specify which regions of the final framebuffer should be updated. For every framebuffer, the compositor will composite its corresponding regions into the boxes of the final framebuffer. It will update the whole final framebuffer if this argument is `None`.
    fn composite<'a, U: IntoIterator<Item = R> + Clone, P: 'a + Pixel>(
        &mut self,
        src_fbs: impl IntoIterator<Item = FrameBufferUpdates<'a, P>>,
        dest_fb: &mut FrameBuffer<P>,
        bounding_boxes: U
    ) -> Result<(), &'static str>;
}


/// The framebuffers to be composited together with their target positions.
pub struct FrameBufferUpdates<'a, P: Pixel> {
    /// The framebuffer to be composited.
    pub framebuffer: &'a FrameBuffer<P>,
    /// The coordinate where the source `framebuffer` should be composited into the destination framebuffer,
    /// which is relative to the top-left point of the destination framebuffer. 
    pub coordinate: Coord,
}

/// A `BlendableRegion` is an abstract region (i.e., shape, bounding box) 
/// that can optimize the blending of one framebuffer's pixels into another framebuffer's pixels,
/// according to the nature of each region (e.g., a single point, a rectangle, etc).
/// This allows a compositor to blend the pixel contents in only a subset of bounding boxes and 
/// composite them to a destination framebuffer rather than doing so for the whole framebuffer,
/// which vastly improves performance.
pub trait BlendableRegion {
    /// Blends the pixels in the source framebuffer `src_fb` into the pixels in the destination framebuffer `dest_fb`.
    /// The `dest_coord` is the coordinate relative to the top-left of the destination buffer.
    fn blend_buffers<P: Pixel>(
        &self, 
        src_fb: &FrameBuffer<P>, 
        dest_fb: &mut FrameBuffer<P>, 
        dest_coord: Coord,        
    ) -> Result<(), &'static str>;
}

impl BlendableRegion for Coord {
    fn blend_buffers<P: Pixel>(
        &self, 
        src_fb: &FrameBuffer<P>,
        dest_fb: &mut FrameBuffer<P>, 
        dest_coord: Coord,        
    ) -> Result<(), &'static str>{
        let relative_coord = self.clone() - dest_coord;
        if let Some(pixel) = src_fb.get_pixel(relative_coord) {
            dest_fb.draw_pixel(self.clone(), pixel);
        }
        Ok(())
    }
}

impl BlendableRegion for Rectangle {
    fn blend_buffers<P: Pixel>(
        &self, 
        src_fb: &FrameBuffer<P>, 
        dest_fb: &mut FrameBuffer<P>,
        dest_coord: Coord,
    ) -> Result<(), &'static str> {
        let (dest_width, dest_height) = dest_fb.get_size();
        let (src_width, src_height) = src_fb.get_size();
 
        // skip if the updated part is not in the dest framebuffer
        let dest_start = Coord::new(
            core::cmp::max(0, self.top_left.x),
            core::cmp::max(0, self.top_left.y)
        );

        let dest_end = Coord::new(
            core::cmp::min(dest_width as isize, self.bottom_right.x),
            core::cmp::min(dest_height as isize, self.bottom_right.y)
        );
        if dest_end.x < 0
            || dest_start.x > dest_width as isize
            || dest_end.y < 0
            || dest_start.y > dest_height as isize
        {
            return Ok(());
        }
                
        // skip if the updated part is not in the source framebuffer
        let coordinate_start = dest_start - dest_coord;
        let coordinate_end = dest_end - dest_coord;
        if coordinate_end.x < 0
            || coordinate_start.x > src_width as isize
            || coordinate_end.y < 0
            || coordinate_start.y > src_height as isize
        {
            return Ok(());
        }

        let src_x_start = core::cmp::max(0, coordinate_start.x) as usize;
        let src_y_start = core::cmp::max(0, coordinate_start.y) as usize;

        // draw only the part within the dest buffer
        let width = core::cmp::min(coordinate_end.x as usize, src_width) - src_x_start;
        let height = core::cmp::min(coordinate_end.y as usize, src_height) - src_y_start;

        // copy every line of the block to the dest framebuffer.
        let src_buffer = &src_fb.buffer();
        for i in 0..height {
            let src_start = Coord::new(src_x_start as isize, (src_y_start + i) as isize);
            let src_start_index = match src_fb.index_of(src_start) {
                Some(index) => index,
                None => {continue;}
            };
            let src_end_index = src_start_index + width;
            let dest_start = src_start + dest_coord;
            let dest_start_index =  match dest_fb.index_of(dest_start) {
                Some(index) => index,
                None => {continue;}
            };
            dest_fb.composite_buffer(&(src_buffer[src_start_index..src_end_index]), dest_start_index as usize);
        }

        Ok(())
    }
}
