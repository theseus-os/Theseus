//! This crate defines a framebuffer compositor.
//! A framebuffer compositor composites a list of framebuffers into a single destination framebuffer.
//! The coordinate of a framebuffer represents its origin (top-left point).
//!
//! # Cache
//! The compositor caches framebuffer rows for better performance. 
//!
//! First, it divides an incoming framebuffer into every 16(CACHE_BLOCK_HEIGHT) rows and deals with these row-range one by one. The pixels in a row-range is a continuous array of length `16 * frame_buffer_width` so that we can compute its hash to cache the content. 
//!
//! In the next step, for every 16 rows, the compositor checks if the pixel array of the 16 rows are already cached. It ignores row-ranges that do not overlap with the bounding box to be updated. If a pixel array is not cached, the compositor will refresh the pixels within the bounding box and cache the 16 rows.
//!
//! In order to cache some rows of the source framebuffer, the compositor needs to cache its contents, its location in the destination framebuffer and its width and height. It's basically a rectangle region in the destination framebuffer and we define a structure `CacheBlock` to represent it.


#![no_std]

extern crate alloc;
extern crate compositor;
extern crate frame_buffer;
extern crate spin;
#[macro_use]
extern crate lazy_static;
extern crate hashbrown;
extern crate shapes;

use alloc::collections::BTreeMap;
use alloc::vec::{Vec};
use core::hash::{Hash, Hasher, BuildHasher};
use hashbrown::hash_map::{DefaultHashBuilder};
use compositor::{Compositor, FrameBufferUpdates, CompositableRegion};
use frame_buffer::{FrameBuffer, Pixel};
use shapes::{Coord, Rectangle};
use spin::Mutex;
use core::ops::Range;

/// The height of a cache block. In every iteration the compositor will deal of 16 rows and cache them.
pub const CACHE_BLOCK_HEIGHT:usize = 16;

lazy_static! {
    /// The instance of the framebuffer compositor.
    pub static ref FRAME_COMPOSITOR: Mutex<FrameCompositor> = Mutex::new(
        FrameCompositor{
            caches: BTreeMap::new()
        }
    );
}

/// Metadata that describes a cached block. It represents the cache of some rows in the source framebuffer updated before. It's basically a rectangle region and its contents in the destination framebuffer and is independent from the source framebuffer after cached.
/// `block` is a rectangle region in the destination framebuffer occupied by the updated rows in the source framebuffer. We need to cache these information because if an old cache block overlap with some new framebuffer rows to be updated, the compositor should remove the old one since part of the region will change.
/// `content_hash` is the hash of pixels in the source framebuffer rows to be cached. A cache block is identical to some new framebuffer rows to be updated if they share the same `content_hash` and `width`.
pub struct CacheBlock {
    /// the rectangle region of this cache block. It specifies the size and location of the block
    block: Rectangle,
    /// The hash of the pixel array in the block.
    content_hash: u64,
}

impl CacheBlock {
    /// Checks if a cache block overlaps with another one
    pub fn overlaps_with(&self, cache: &CacheBlock) -> bool {
        self.contains_corner(cache) || cache.contains_corner(self)
    }

    /// checks if the coordinate is within the block
    fn contains(&self, coordinate: Coord) -> bool {
        return coordinate.x >= self.block.top_left.x
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
    // Cache of updated framebuffers
    caches: BTreeMap<Coord, CacheBlock>,
}

impl FrameCompositor {
    /// Checks if some rows of a framebuffer is cached.
    /// # Arguments
    /// * `row_pixels`: the continuous pixels in the rows.
    /// * `dest_coord`: the location of the first pixel in the destination framebuffer.
    /// * `width`: the width of the rows
    ///
    fn is_cached<P: Pixel>(&self, row_pixels: &[P], dest_coord: &Coord, width: usize) -> bool {
        match self.caches.get(dest_coord) {
            Some(cache) => {
                // The same hash and width means the cache block is identical to the row pixels. We do not check the height because if the hashes are the same, the number of pixels, namely `width * height` must be the same.
                return cache.content_hash == hash(row_pixels) && (cache.block.bottom_right.x - cache.block.top_left.x) as usize == width
            }
            None => return false,
        }
    }

    /// This function will return if several continuous rows in the framebuffer is cached.
    /// If the answer is no, it will remove the old cache blocks overlaps with the rows and cache the rows as a new cache block.
    /// # Arguments
    /// * `src_fb`: the updated source framebuffer.
    /// * `dest_coord`: the position of the source framebuffer(top-left corner) relative to the destination framebuffer(top-left corner).
    /// * `src_fb_row_range`: the index range of rows in the source framebuffer to check and cache.
    fn check_and_cache<P: Pixel>(
        &mut self, 
        src_fb: &FrameBuffer<P>, 
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
        if self.is_cached(&pixel_slice, &coordinate_start, src_width) {
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

    /// Returns the row index range in the framebuffer that may be cached before as cache blocks and overlap with the bounding box. This methods extends the row range of the bounding box because the compositor should deal with every `CACHE_BLOCK_HEIGHT` rows.
    /// # Arguments
    /// * `dest_coord`: the position of the framebuffer relative to the top-left of the destination framebuffer where the source framebuffer will be composited to.
    /// * `dest_bounding_box`: the compositable region to be composited.
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
        let mut cache_row_end = (relative_row_end as usize / CACHE_BLOCK_HEIGHT + 1) * CACHE_BLOCK_HEIGHT;

        cache_row_end = core::cmp::min(cache_row_end, src_fb_height);

        return cache_row_start..cache_row_end
    }

}

impl Compositor for FrameCompositor {
    fn composite<'a, B: CompositableRegion + Clone, P: 'a + Pixel>(
        &mut self,
        src_fbs: impl IntoIterator<Item = FrameBufferUpdates<'a, P>>,
        dest_fb: &mut FrameBuffer<P>,
        dest_bounding_boxes: impl IntoIterator<Item = B> + Clone,
    ) -> Result<(), &'static str> {
        let mut box_iter = dest_bounding_boxes.clone().into_iter();
        if box_iter.next().is_none() {
            for frame_buffer_updates in src_fbs.into_iter() {
                let src_fb = frame_buffer_updates.framebuffer;
                let coordinate = frame_buffer_updates.coordinate;
                // Update the whole screen if the caller does not specify the blocks
                let (src_width, src_height) = frame_buffer_updates.framebuffer.get_size();
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
            for frame_buffer_updates in src_fbs.into_iter() {
                //let mut updated_blocks = Vec::new();
                for bounding_box in dest_bounding_boxes.clone() {
                    let src_fb = frame_buffer_updates.framebuffer;
                    let coordinate = frame_buffer_updates.coordinate;
                    let (_, height) = src_fb.get_size();
                    let mut row_range = self.get_cache_row_range(coordinate, &bounding_box, height);
                    // let cache_block_size = CACHE_BLOCK_HEIGHT * width;

                    loop {
                        if row_range.start >= row_range.end {
                            break;
                        }
                        let cache_range = row_range.start..(row_range.start + CACHE_BLOCK_HEIGHT);
                        // check cache if the bounding box is not a single pixel
                        if bounding_box.size() > 1 {
                            if self.check_and_cache(src_fb, coordinate, &cache_range)? {
                                 row_range.start += CACHE_BLOCK_HEIGHT;
                                 continue;
                            }
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