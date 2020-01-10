//! This crate defines a trait of `Compositor`  .
//! A compositor composites a list of buffers to a single buffer.

#![no_std]

extern crate frame_buffer;
extern crate shapes;

use core::iter::IntoIterator;

use frame_buffer::{FrameBuffer, Pixel};
use shapes::{Coord, Rectangle};

/// A compositor composites (combines or blends) a series of "source" framebuffers onto a single "destination" framebuffer. 
/// The type parameter `B` allows a compositor to support multiple types of regions or "bounding boxes", 
/// given by the trait bound `CompositableRegion`.
pub trait Compositor {
    /// Composites the framebuffers in the list of source framebuffers `src_fbs` into the destination framebuffer `dest_fb`.
    ///
    /// # Arguments
    /// * `src_fbs`: an iterator over the source framebuffers to be composited, along with where in the `dest_fb` they should be composited. 
    /// * `dest_fb`: the destination framebuffer that will hold the composited source framebuffers.
    /// * `bounding_boxes`: an iterator over bounding boxes that specify which regions of the destination framebuffer should be updated. 
    ///    In the iteration of every source framebuffer, the compositor will traverse all the bounding boxes relative to the destination framebuffer, get the part of the source framebuffer in every bounding box when the source is composited to the destination one, and blend the part with the bounded region in the destination.
    /// For example, if the window manager wants to draw a line in the top window, `src_fbs` would be the framebuffers of all the windows and their location in a bottom-top order, and the `bounding_boxes` is an iterator over the pixels of the line relative to the top-left corner of the screen. For every window, the compositor will update the pixels at the location of line in the screen from the bottom window to the top one.
    fn composite<'a, B: CompositableRegion + Clone, P: 'a + Pixel>(
        &mut self,
        src_fbs: impl IntoIterator<Item = FrameBufferUpdates<'a, P>>,
        dest_fb: &mut FrameBuffer<P>,
        bounding_boxes: impl IntoIterator<Item = B> + Clone,
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

/// A `CompositableRegion` is an abstract region (i.e., a bounding box) 
/// that can optimize the compositing (blending) of one framebuffer into another framebuffer
/// according to the specifics of the region's shape. 
/// For example, a single 2-D point (`Coord`) offers no real room for optimization 
/// because only one pixel will be composited,
/// but a rectangle **does** allow for optimization, as a large chunk of pixels can be composited all at once.
/// 
/// In addition, a `CompositableRegion` makes it easier for a compositor to only composite pixels in a subset of a given source framebuffer
/// rather than forcing it to composite the whole framebuffer, which vastly improves performance.
pub trait CompositableRegion {
    /// Returns the start and end rows in the framebuffer that may be cached and overlap with the region. The row range is usually equal to or larger than the region because the framebuffers are cached as every `cache_block_height` rows.
    /// # Arguments
    /// * `framebuffer`: the source framebuffer that the compositable region is in.
    /// * `coordinate`: the position relative to the top-left of the destination framebuffer where the source framebuffer will be composited to.
    /// * `cache_block_height`: the height of every cache block of the framebuffer.
    fn get_cache_row_range<P: Pixel>(    
        &self,
        framebuffer: &FrameBuffer<P>, 
        coordinate: Coord, 
        cache_block_height: usize,
    ) -> (usize, usize);

    /// Returns the intersection of the compositable region and the continuous rows
    /// # Arguments
    /// * `row_start`: the index of the start row
    /// * `coordinate`: the position relative to the top-left of the destination framebuffer where the source framebuffer will be composited to.
    /// * `row_num`: the number of rows
    fn intersect_rows(&self, row_start: usize, coordinate: Coord, row_num: usize) -> Self;

    /// Returns the number of pixels in the region.
    fn size(&self) -> usize;

    /// Blends the pixels in the source framebuffer `src_fb` into the pixels in the destination framebuffer `dest_fb`.
    /// The `dest_coord` is the coordinate in the destination buffer (relative to its top-left corner)
    /// where the `src_fb` will be composited into (starting at the `src_fb`'s top-left corner).
    fn blend_buffers<P: Pixel>(
        &self, 
        src_fb: &FrameBuffer<P>, 
        dest_fb: &mut FrameBuffer<P>, 
        dest_coord: Coord,        
    ) -> Result<(), &'static str>;
}

impl CompositableRegion for Coord {
    fn get_cache_row_range<P: Pixel>(    
        &self,
        framebuffer: &FrameBuffer<P>, 
        coordinate: Coord,
        cache_block_height: usize,
    ) -> (usize, usize) {
        let relative_coord = *self - coordinate;
        let (_, height) = framebuffer.get_size();
        if relative_coord.y >= 0 && relative_coord.y < height as isize {
            let row_start = relative_coord.y as usize / cache_block_height * cache_block_height;
            return (row_start, row_start + cache_block_height);
        } else {
            return (0, 0);
        }
    }
 
    fn intersect_rows(&self, _row_start: usize, _coordinate: Coord, _row_num: usize) -> Coord {
        return self.clone()
    }

    #[inline]
    fn size(&self) -> usize {
        1
    }

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

impl CompositableRegion for Rectangle {
    fn get_cache_row_range<P: Pixel>(    
        &self,
        framebuffer: &FrameBuffer<P>, 
        coordinate: Coord, 
        cache_block_height: usize,
    ) -> (usize, usize) {
        let relative_area = *self - coordinate;
        let (width, height) = framebuffer.get_size();

        let start_x = core::cmp::max(relative_area.top_left.x, 0);
        let end_x = core::cmp::min(relative_area.bottom_right.x, width as isize);
        if start_x >= end_x {
            return (0, 0);
        }
        
        let start_y = core::cmp::max(relative_area.top_left.y, 0);
        let end_y = core::cmp::min(relative_area.bottom_right.y, height as isize);
        if start_y >= end_y {
            return (0, 0);
        }
        let start_index = start_y as usize / cache_block_height;
        let end_index = end_y as usize / cache_block_height + 1;
        
        return (start_index * cache_block_height, end_index * cache_block_height)
    }

    fn intersect_rows(&self, row_start: usize, coordinate: Coord, row_num: usize) -> Rectangle {
        return Rectangle {
            top_left: Coord::new(
                self.top_left.x,
                core::cmp::max((row_start) as isize + coordinate.y, self.top_left.y),
            ),
            bottom_right: Coord::new(
                self.bottom_right.x,
                core::cmp::min((row_start + cache_block_height) as isize + coordinate.y, self.bottom_right.y)
            )
        };
    }

    #[inline]
    fn size(&self) -> usize {
        (self.bottom_right.x - self.top_left.x) as usize * (self.bottom_right.y - self.top_left.y) as usize
    }

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
