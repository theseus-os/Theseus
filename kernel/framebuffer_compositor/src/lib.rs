//! This crate defines a framebuffer compositor.
//!
//! A framebuffer compositor composites a list of framebuffers into a single destination framebuffer.
//! The coordinate system within a framebuffer is expressed relative to its origin, i.e., the top-left point.
//!
//! # Cache
//! The compositor caches groups of framebuffer rows for better performance. 
//!
//! First, it divides each framebuffer into ranges of rows called "blocks" which are `CACHE_BLOCK_HEIGHT` rows in height,
//! and deals with these row ranges one by one. 
//! The pixels in each block's row range are a contiguous array of length `CACHE_BLOCK_HEIGHT * framebuffer_width`,
//! and the cache key is the hash value of that pixel array.
//!
//! In the next step, for every `CACHE_BLOCK_HEIGHT` rows, the compositor checks if the pixel array is are already cached.
//! It ignores row ranges that do not overlap with the given bounding box to be updated.
//! If a pixel array is not cached, the compositor will refresh the pixels within the bounding box and cache those `CACHE_BLOCK_HEIGHT` rows.
//!
//! In order to cache a range of rows from the source framebuffer, the compositor needs to cache its contents, its location in the destination framebuffer, and its size.
//! The cache is basically a rectangular region in the destination framebuffer, and we define the structure `CacheBlock` to represent that cached region.

#![no_std]

extern crate alloc;
extern crate compositor;
extern crate framebuffer;
extern crate spin;
extern crate hashbrown;
extern crate shapes;

use alloc::collections::BTreeMap;
use alloc::vec::{Vec};
use core::hash::{Hash, Hasher, BuildHasher};
use hashbrown::hash_map::{DefaultHashBuilder};
use compositor::{Compositor, FramebufferUpdates, CompositableRegion};
use framebuffer::{Framebuffer, Pixel};
use shapes::{Coord, Rectangle};
use spin::Mutex;
use core::ops::Range;

/// The height of a cache block. In every iteration the compositor will deal with groups of 16 rows and cache them.
pub const CACHE_BLOCK_HEIGHT: usize = 16;

/// The instance of the framebuffer compositor.
pub static FRAME_COMPOSITOR: Mutex<FrameCompositor> = Mutex::new(
    FrameCompositor{
        caches: BTreeMap::new()
    }
);

/// A `CacheBlock` represents the cached (previously-composited) content of a range of rows in the source framebuffer. 
/// It specifies the rectangular region in the destination framebuffer and the hash.
/// Once cached, a `CacheBlock` block is independent of the source framebuffer it came from.
/// `content_hash` is the hash value of the actual pixel contents in the cached block. A cache block is identical to some new framebuffer rows to be updated if they share the same `content_hash`, location and width.
pub struct CacheBlock {
    /// The rectanglular region in the destination framebuffer occupied by the cached rows in the source framebuffer. 
    /// We need this information because if an old cache block overlaps with some new framebuffer rows to be updated, 
    /// the compositor should remove the old one since part of that region will change.
    block: Rectangle,
    /// The hash value of the actual pixel contents in the cached block.
    content_hash: u64,
}

impl CacheBlock {
    /// Checks if a cache block overlaps with another one
    pub fn overlaps_with(&self, cache: &CacheBlock) -> bool {
        self.contains_corner(cache) || cache.contains_corner(self)
    }

    /// checks if the coordinate is within the block
    fn contains(&self, coordinate: Coord) -> bool {
        coordinate.x >= self.block.top_left.x
            && coordinate.x < self.block.bottom_right.x
            && coordinate.y >= self.block.top_left.y
            && coordinate.y < self.block.bottom_right.y
    }

    /// checks if this block contains any of the four corners of another cache block.
    fn contains_corner(&self, cache: &CacheBlock) -> bool {
        self.contains(cache.block.top_left)
            || self.contains(cache.block.top_left + (cache.block.bottom_right.x - cache.block.top_left.x - 1, 0))
            || self.contains(cache.block.top_left + (0, cache.block.bottom_right.y - cache.block.top_left.y - 1))
            || self.contains(cache.block.bottom_right - (1, 1))
    }
}

/// The framebuffer compositor structure.
/// It caches framebuffer rows since last update as soft states for better performance.
pub struct FrameCompositor {
    // Cache of updated framebuffers before
    caches: BTreeMap<Coord, CacheBlock>,
}

impl FrameCompositor {
    /// Checks if some rows of a framebuffer are cached.
    /// # Arguments
    /// * `row_pixels`: the continuous pixels in the rows.
    /// * `dest_coord`: the location of the first pixel in the destination framebuffer.
    /// * `width`: the width of the rows
    ///
    fn is_cached<P: Pixel>(&self, row_pixels: &[P], dest_coord: &Coord, width: usize) -> bool {
        match self.caches.get(dest_coord) {
            Some(cache) => {
                // The same hash and width means the cache block is identical to the row pixels.
                // We do not check the height because if the hashes are the same, the number of pixels, namely `width * height` must be the same.
                cache.content_hash == hash(row_pixels) && (cache.block.bottom_right.x - cache.block.top_left.x) as usize == width
            }
            None => false
        }
    }

    /// This function will return true if several continuous rows in the framebuffer are cached.
    /// If false, i.e. the given `row_range` is not in the cache, this function will remove 
    /// the old cached blocks that overlap with the rows in the given `src_fb_row_range` and cache those rows as a new cache block.
    /// # Arguments
    /// * `src_fb`: the updated source framebuffer.
    /// * `dest_coord`: the position of the source framebuffer (its top-left corner) relative to the destination framebuffer's top-left corner.
    /// * `src_fb_row_range`: the range of rows in the source framebuffer to check and cache.
    fn check_and_cache<P: Pixel>(
        &mut self, 
        src_fb: &Framebuffer<P>, 
        dest_coord: Coord, 
        src_fb_row_range: &Range<usize>,
    ) -> Result<bool, &'static str> {
        let (src_width, src_height) = src_fb.get_size();
        let src_buffer_len = src_width * src_height;

        // The start pixel of the rows
        let start_index = src_width * src_fb_row_range.start;
        let coordinate_start = dest_coord + (0, src_fb_row_range.start as isize);

        // The end pixel of the rows
        let end_index = src_width * src_fb_row_range.end;
        
        let pixel_slice = &src_fb.buffer()[start_index..core::cmp::min(end_index, src_buffer_len)];
        
        // Skip if the rows are already cached
        if self.is_cached(pixel_slice, &coordinate_start, src_width) {
            return Ok(true);
        }

        // remove overlapped caches
        let new_cache = CacheBlock {
            block: Rectangle {
                top_left: coordinate_start,
                bottom_right: coordinate_start + (src_width as isize, (pixel_slice.len() / src_width) as isize)
            },
            content_hash: hash(pixel_slice),
        };
        let keys: Vec<_> = self.caches.keys().cloned().collect();
        for key in keys {
            if let Some(cache) = self.caches.get_mut(&key) {
                if cache.overlaps_with(&new_cache) {
                    self.caches.remove(&key);
                }
            };
        }

        self.caches.insert(coordinate_start, new_cache);        
        Ok(false)
    }

    /// Returns the range of rows in the source framebuffer that were (1) previously cached as cache blocks
    /// and (2) overlap with the given `dest_bounding_box`. 
    /// This methods extends the row range of the given bounding box because the compositor deals with chunks of `CACHE_BLOCK_HEIGHT` rows.
    /// # Arguments
    /// * `dest_coord`: the position in the destination framebuffer (relative to its top-left corner)
    ///    to where the source framebuffer will be composited.
    /// * `dest_bounding_box`: the region of the destination framebuffer that should be composited.
    /// * `src_fb_height`: the height of the source framebuffer.
    fn get_cache_row_range<B: CompositableRegion>(
        &self,
        dest_coord: Coord,
        dest_bounding_box: &B,
        src_fb_height: usize,
    ) -> Range<usize> {
        let abs_row_range = dest_bounding_box.row_range();
        let mut relative_row_start = abs_row_range.start - dest_coord.y;
        let mut relative_row_end = abs_row_range.end - dest_coord.y;

        relative_row_start = core::cmp::max(relative_row_start, 0);
        relative_row_end = core::cmp::min(relative_row_end, src_fb_height as isize);

        if relative_row_start >= relative_row_end {
            return 0..0;
        }
        
        let cache_row_start = relative_row_start as usize / CACHE_BLOCK_HEIGHT * CACHE_BLOCK_HEIGHT;
        let mut cache_row_end = ((relative_row_end - 1) as usize / CACHE_BLOCK_HEIGHT + 1) * CACHE_BLOCK_HEIGHT;

        cache_row_end = core::cmp::min(cache_row_end, src_fb_height);

        cache_row_start..cache_row_end
    }

}

impl Compositor for FrameCompositor {
    fn composite<'a, B: CompositableRegion + Clone, P: 'a + Pixel>(
        &mut self,
        src_fbs: impl IntoIterator<Item = FramebufferUpdates<'a, P>>,
        dest_fb: &mut Framebuffer<P>,
        dest_bounding_boxes: impl IntoIterator<Item = B> + Clone,
    ) -> Result<(), &'static str> {
        let mut box_iter = dest_bounding_boxes.clone().into_iter();
        if box_iter.next().is_none() {
            for framebuffer_updates in src_fbs.into_iter() {
                let src_fb = framebuffer_updates.src_framebuffer;
                let coordinate = framebuffer_updates.coordinate_in_dest_framebuffer;
                // Update the whole screen if the caller does not specify the blocks
                let (src_width, src_height) = framebuffer_updates.src_framebuffer.get_size();
                // let block_number = (src_height - 1) / CACHE_BLOCK_HEIGHT + 1;
                let area = Rectangle {
                    top_left: coordinate,
                    bottom_right: coordinate + (src_width as isize, src_height as isize)
                };
                let mut row_start = 0;
                loop {
                    if row_start >= src_height {
                        break;
                    }
                    let cache_range = row_start..(row_start + CACHE_BLOCK_HEIGHT);
                    if !self.check_and_cache(src_fb, coordinate, &cache_range)? {
                        area.blend_buffers(
                            src_fb,
                            dest_fb,
                            coordinate,
                            cache_range,
                        )?;
                    }
                    row_start += CACHE_BLOCK_HEIGHT;
                }
            }
        } else {
            for framebuffer_updates in src_fbs.into_iter() {
                //let mut updated_blocks = Vec::new();
                for bounding_box in dest_bounding_boxes.clone() {
                    let src_fb = framebuffer_updates.src_framebuffer;
                    let coordinate = framebuffer_updates.coordinate_in_dest_framebuffer;
                    let (_, height) = src_fb.get_size();
                    let mut row_range = self.get_cache_row_range(coordinate, &bounding_box, height);
                    // let cache_block_size = CACHE_BLOCK_HEIGHT * width;

                    loop {
                        if row_range.start >= row_range.end {
                            break;
                        }
                        let cache_range = row_range.start..(row_range.start + CACHE_BLOCK_HEIGHT);
                        // check cache if the bounding box is not a single pixel
                        if bounding_box.size() > 1 && self.check_and_cache(src_fb, coordinate, &cache_range)? {
                            row_range.start += CACHE_BLOCK_HEIGHT;
                            continue;
                        };
                        bounding_box.blend_buffers(
                            src_fb,
                            dest_fb,
                            coordinate,
                            cache_range,
                        )?;
                        row_range.start += CACHE_BLOCK_HEIGHT;
                    } 
                }
            }
        }

        Ok(())
    }
}

/// Gets the hash of an item
fn hash<T: Hash>(item: T) -> u64 {
    let builder = DefaultHashBuilder::default();
    let mut hasher = builder.build_hasher();
    item.hash(&mut hasher);
    hasher.finish()
}
