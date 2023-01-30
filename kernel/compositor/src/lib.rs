//! This crate defines a trait of `Compositor`  .
//! A compositor composites a list of sources buffers to a single destination buffer.

#![allow(clippy::range_plus_one)]
#![no_std]

extern crate framebuffer;
extern crate shapes;

use core::iter::IntoIterator;

use framebuffer::{Framebuffer, Pixel};
use shapes::{Coord, Rectangle};
use core::ops::Range;

/// A compositor composites (combines or blends) a series of "source" framebuffers onto a single "destination" framebuffer. 
/// The type parameter `B` allows a compositor to support multiple types of regions or "bounding boxes", 
/// given by the trait bound `CompositableRegion`.
pub trait Compositor {
    /// Composites the framebuffers in the list of source framebuffers `src_fbs` onto the destination framebuffer `dest_fb`.
    ///
    /// # Arguments
    /// * `src_fbs`: an iterator over the source framebuffers to be composited, 
    ///    along with where in the `dest_fb` they should be composited. 
    /// * `dest_fb`: the destination framebuffer that will hold the source framebuffers to be composited.
    /// * `dest_bounding_boxes`: an iterator over bounding boxes that specify which regions
    ///    in the destination framebuffer should be updated. 
    ///    For each source framebuffer in `src_fbs`, the compositor will iterate over every bounding box
    ///    and find the corresponding region in that source framebuffer and then blend that region into the destination.
    /// 
    /// For example, if the window manager wants to draw a new partially-transparent window,
    /// it will pass the framebuffers for all existing windows plus the new window (in bottom-to-top order)
    /// to the compositor, in the argument `src_fbs`. 
    /// The `dest_fb` would be the final framebuffer mapped to the display device (screen memory),
    /// and the `bounding_boxes` would be an iterator over just a single region in the final framebuffer
    /// where that new window will be located. 
    /// When the source framebuffers are composited from bottom to top, the compositor will redraw the region of every source framebuffer
    /// that intersects with that bounding box.
    ///
    /// For another example, suppose the window manager wants to draw a transparent mouse pointer on top of all windows.
    /// It will pass the framebuffers of existing windows as well as a top framebuffer that contains the mouse pointer image.
    /// In this case, the `bounding_boxes` could be the coordinates of all individual pixels in the mouse pointer image
    /// (expressed as coordinates in the final framebuffer).
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

    /// Returns the range of rows covered by this region, 
    /// given as row indices where row `0` is the top row in the region.
    fn row_range(&self) -> Range<isize>;

    /// Blends the pixels in the source framebuffer `src_fb` within the range of rows (`src_fb_row_range`) 
    /// into the pixels in the destination framebuffer `dest_fb`.
    /// The `dest_coord` is the coordinate in the destination buffer (relative to its top-left corner)
    /// where the `src_fb` will be composited (starting at the `src_fb`'s top-left corner).
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
        let relative_coord = *self - dest_coord;
        if let Some(pixel) = src_fb.get_pixel(relative_coord) {
            dest_fb.draw_pixel(*self, pixel);
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
            dest_fb.composite_buffer(&src_buffer[src_start_index..src_end_index], dest_start_index);
        }

        Ok(())
    }
}
