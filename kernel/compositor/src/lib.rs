//! This crate defines a trait of `Compositor`  .
//! A compositor composites a list of buffers to a single buffer.

#![no_std]

extern crate frame_buffer;
extern crate shapes;

use core::iter::IntoIterator;

use frame_buffer::{Framebuffer, Pixel};
use shapes::{Coord, Rectangle};
use core::ops::Range;

/// A compositor composites (combines or blends) a series of "source" framebuffers onto a single "destination" framebuffer. 
/// The type parameter `B` allows a compositor to support multiple types of regions or "bounding boxes", 
/// given by the trait bound `CompositableRegion`.
pub trait Compositor {
    /// Composites the framebuffers in the list of source framebuffers `src_fbs` into the destination framebuffer `dest_fb`.
    ///
    /// # Arguments
    /// * `src_fbs`: an iterator over the source framebuffers to be composited, along with where in the `dest_fb` they should be composited. 
    /// * `dest_fb`: the destination framebuffer that will hold the composited source framebuffers.
    /// * `dest_bounding_boxes`: an iterator over bounding boxes that specify which regions of the destination framebuffer should be updated. 
    ///    For each source framebuffer in `src_fbs`, the compositor will iterate over every bounding box relative to the destination framebuffer. It then finds the corresponding region in the source framebuffer when the source is composited to the destination, and blends the region onto the the destination.
    /// 
    /// For example, if the window manager wants to draw a half-transparent window, it will pass the framebuffers of all the existing windows and the new window in a bottom-top order to the compositor as `src_fbs`. The `dest_fb` is the final framebuffer which is mapped to the screen, and the `bounding_boxes` is `Some(area)` in which area is the region in the final framebuffer where the new window will be located. When are source framebuffers are composited from bottom to top, the compositor will redraw the part every source framebuffer in the bounding box.
    ///
    /// In another example, suppose the window manager wants to draw a half-transparent mouse arrow on top of all windows. It will pass the framebuffers of existing windows together with a top framebuffer which covers the screen and contains the arrow. In this case, the `bounding_boxes` are the coordinates of all the pixels in this arrow relative to the final framebuffer. For framebuffers from the bottom one to the top one, the compositor will redraw their pixels at these coordinates relative to the screen(final framebuffer) so that the arrow is displayed on top as half-transparent. 
    fn composite<'a, B: CompositableRegion + Clone, P: 'a + Pixel>(
        &mut self,
        src_fbs: impl IntoIterator<Item = FramebufferUpdates<'a, P>>,
        dest_fb: &mut Framebuffer<P>,
        dest_bounding_boxes: impl IntoIterator<Item = B> + Clone,
    ) -> Result<(), &'static str>;
}


/// A source framebuffer to be composited, along with its target position.
pub struct FramebufferUpdates<'a, P: Pixel> {
    /// The source framebuffer to be composited.
    pub src_framebuffer: &'a Framebuffer<P>,
    /// The coordinate in the destination framebuffer where the source `framebuffer` 
    /// should be composited. 
    /// This coordinate is expressed relative to the top-left corner of the destination framebuffer. 
    pub coordinate_in_dest_framebuffer: Coord,
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
    /// Returns the number of pixels in the region.
    fn size(&self) -> usize;

    /// Returns the range of row index occupied by this region.
    fn row_range(&self) -> Range<isize>;

    /// Blends the pixels in the source framebuffer `src_fb` into the pixels in the destination framebuffer `dest_fb` in the given row range.
    /// The `dest_coord` is the coordinate in the destination buffer (relative to its top-left corner)
    /// where the `src_fb` will be composited into (starting at the `src_fb`'s top-left corner).
    /// `src_fb_row_range` is the index range of rows in the source framebuffer to blend.
    fn blend_buffers<P: Pixel>(
        &self, 
        src_fb: &Framebuffer<P>, 
        dest_fb: &mut Framebuffer<P>, 
        dest_coord: Coord,
        src_fb_row_range: Range<usize>       
    ) -> Result<(), &'static str>;
}

impl CompositableRegion for Coord {
    #[inline]
    fn row_range(&self) -> Range<isize> {
        self.y..self.y + 1
    }

    #[inline]
    fn size(&self) -> usize {
        1
    }

    fn blend_buffers<P: Pixel>(
        &self, 
        src_fb: &Framebuffer<P>,
        dest_fb: &mut Framebuffer<P>, 
        dest_coord: Coord,        
        _src_fb_row_range: Range<usize>,
    ) -> Result<(), &'static str>{
        let relative_coord = self.clone() - dest_coord;
        if let Some(pixel) = src_fb.get_pixel(relative_coord) {
            dest_fb.draw_pixel(self.clone(), pixel);
        }
        Ok(())
    }
}

impl CompositableRegion for Rectangle {
    #[inline]
    fn row_range(&self) -> Range<isize> {
        self.top_left.y..self.bottom_right.y
    }

    #[inline]
    fn size(&self) -> usize {
        (self.bottom_right.x - self.top_left.x) as usize * (self.bottom_right.y - self.top_left.y) as usize
    }

    fn blend_buffers<P: Pixel>(
        &self, 
        src_fb: &Framebuffer<P>, 
        dest_fb: &mut Framebuffer<P>,
        dest_coord: Coord,
        src_fb_row_range: Range<usize>,
    ) -> Result<(), &'static str> {
        let (dest_width, dest_height) = dest_fb.get_size();
        let (src_width, src_height) = src_fb.get_size();

        let start_y = core::cmp::max(src_fb_row_range.start as isize + dest_coord.y, self.top_left.y);
        let end_y = core::cmp::min(src_fb_row_range.end as isize + dest_coord.y, self.bottom_right.y);

        // skip if the updated part is not in the dest framebuffer
        let dest_start = Coord::new(
            core::cmp::max(0, self.top_left.x),
            core::cmp::max(0, start_y)
        );

        let dest_end = Coord::new(
            core::cmp::min(dest_width as isize, self.bottom_right.x),
            core::cmp::min(dest_height as isize, end_y)
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
