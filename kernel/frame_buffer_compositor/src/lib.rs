//! This crate defines a framebuffer compositor.
//! A framebuffer compositor composites a list of framebuffers into a single destination framebuffer.
//! The coordinate of a framebuffer represents its origin (top-left point).
//!
//! # Cache
//! The compositor caches framebuffer rows for better performance. 
//!
//! First, it divides every incoming framebuffer into several row ranges. Every row range has 16 rows except for the last one. The pixels in a row range is a continuous array so that we can compute its hash to compare the content of two row ranges.
//!
//! In the next step, the compositor checks if the contents of a framebuffer within every row range is already cached. It ignores those do not overlap the bounding box to be updated. For rows in a range that have not been cached, the compositor will refresh the part of these rows within the bounding box.
//!
//! Once a row range is updated, the compositor will remove all the existing caches overlap with it and cache the new one. It computes the hash of the pixels in the row range and wrap it with the size and location of the pixels as a cache block.

#![no_std]

extern crate alloc;
extern crate compositor;
extern crate frame_buffer;
extern crate spin;
#[macro_use]
extern crate log;
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

/// Metadata that describes the cached block. 
pub struct CacheBlock {
    /// The coordinate of the block where it is rendered to the destination framebuffer.
    coordinate: Coord,
    /// The hash of the content in the block. It is the hash of continuous pixels.
    content_hash: u64,
    /// The width of the block
    width: usize,
    /// The height of the block
    height: usize,
}

impl CacheBlock {
    /// Checks if a block cache overlaps with another one
    pub fn overlaps_with(&self, cache: &CacheBlock) -> bool {
        self.contains_corner(cache) || cache.contains_corner(self)
    }

    /// checks if the coordinate is within the block
    fn contains(&self, coordinate: Coord) -> bool {
        return coordinate.x >= self.coordinate.x
            && coordinate.x < self.coordinate.x + self.width as isize
            && coordinate.y >= self.coordinate.y
            && coordinate.y < self.coordinate.y + self.height as isize;
    }

    /// checks if this block contains any of the four corners of another `cache`.
    fn contains_corner(&self, cache: &CacheBlock) -> bool {
        self.contains(cache.coordinate)
            || self.contains(cache.coordinate + (cache.width as isize - 1, 0))
            || self.contains(cache.coordinate + (0, cache.height as isize - 1))
            || self.contains(cache.coordinate + (cache.width as isize - 1, cache.height as isize - 1))
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
    /// * `coordinate`: the location of the first pixel.
    /// * `width`: the width of the rows
    ///
    fn is_cached<P: Pixel>(&self, row_pixels: &[P], coordinate: &Coord, width: usize) -> bool {
        match self.caches.get(coordinate) {
            Some(cache) => {
                // The same hash and width means the two cache blocks are the same. We do not check the height because if the hashes are the same, the number of pixels, namely `width * height` must be the same.
                return cache.content_hash == hash(row_pixels) && cache.width == width
            }
            None => return false,
        }
    }

    /// This function will return if several continuous rows in the framebuffer is cached.
    /// If the answer is no, it will remove the cache overlaps with the rows and cache the rows
    /// # Arguments
    /// * `src_fb`: the updated source framebuffer.
    /// * `coordinate`: the position of the source framebuffer relative to the destination framebuffer.
    /// * `row_start`: start row index to be checked.
    /// * `row_num`: the number of rows to be checked
    fn check_and_cache<P: Pixel>(
        &mut self, 
        src_fb: &FrameBuffer<P>, 
        coordinate: Coord, 
        row_start: usize,
        row_num: usize,
    ) -> Result<bool, &'static str> {
        let (src_width, src_height) = src_fb.get_size();
        let src_buffer_len = src_width * src_height;

        // The start pixel of the rows
        let start_index = src_width * row_start;
        let coordinate_start = coordinate + (0, row_start as isize);

        // The end pixel of the rows
        let end_index = start_index + row_num * src_width;
        
        let pixel_slice = &src_fb.buffer()[start_index..core::cmp::min(end_index, src_buffer_len)];
        
        // Skip if the rows are already cached
        if self.is_cached(&pixel_slice, &coordinate_start, src_width) {
            return Ok(true);
        }

        // remove overlapped caches
        let new_cache = CacheBlock {
            content_hash: hash(pixel_slice),
            coordinate: coordinate_start,
            width: src_width,
            height: pixel_slice.len() / src_width
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

    /// Returns the start and end rows in the framebuffer that may be cached and overlap with the region. The row range is usually equal to or larger than the region because the framebuffers are cached as cache blocks and every cache block has a fixed `CACHE_BLOCK_HEIGHT` rows.
    /// # Arguments
    /// * `coordinate`: the position of the framebuffer relative to the top-left of the destination framebuffer where the source framebuffer will be composited to.
    /// * `bounding_box`: the compositable region to be composited
    /// * `fb_height`: the height of the framebuffer.
    fn get_cache_row_range<B: CompositableRegion>(
        &self,
        coordinate: Coord,
        bounding_box: &B,
        fb_height: usize,
    ) -> (usize, usize) {
        let (abs_row_start, abs_row_end) = bounding_box.row_range();
        let mut relative_row_start = abs_row_start - coordinate.y;
        let mut relative_row_end = abs_row_end - coordinate.y;

        relative_row_start = core::cmp::max(relative_row_start, 0);
        relative_row_end = core::cmp::min(relative_row_end, fb_height as isize);

        if relative_row_start >= relative_row_end {
            return (0, 0);
        }
        
        let cache_row_start = relative_row_start as usize / CACHE_BLOCK_HEIGHT * CACHE_BLOCK_HEIGHT;
        let mut cache_row_end = (relative_row_end as usize / CACHE_BLOCK_HEIGHT + 1) * CACHE_BLOCK_HEIGHT;

        cache_row_end = core::cmp::min(cache_row_end, fb_height);

        return (cache_row_start, cache_row_end)
    }

}

impl Compositor for FrameCompositor {
    fn composite<'a, B: CompositableRegion + Clone, P: 'a + Pixel>(
        &mut self,
        src_fbs: impl IntoIterator<Item = FrameBufferUpdates<'a, P>>,
        dest_fb: &mut FrameBuffer<P>,
        bounding_boxes: impl IntoIterator<Item = B> + Clone,
    ) -> Result<(), &'static str> {
        let mut box_iter = bounding_boxes.clone().into_iter();
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
                    if !self.check_and_cache(src_fb, coordinate, row_start, CACHE_BLOCK_HEIGHT)? {
                        area.blend_buffers(
                            src_fb,
                            dest_fb,
                            coordinate,
                            row_start,
                            CACHE_BLOCK_HEIGHT,
                        )?;
                    }
                    row_start += CACHE_BLOCK_HEIGHT;
                }
            }
        } else {
            for frame_buffer_updates in src_fbs.into_iter() {
                //let mut updated_blocks = Vec::new();
                for bounding_box in bounding_boxes.clone() {
                    let src_fb = frame_buffer_updates.framebuffer;
                    let coordinate = frame_buffer_updates.coordinate;
                    let (_, height) = src_fb.get_size();
                    let (mut row_start, row_end) = self.get_cache_row_range(coordinate, &bounding_box, height);
                    // let cache_block_size = CACHE_BLOCK_HEIGHT * width;

                    loop {
                        if row_start >= row_end {
                            break;
                        }  
                        // check cache if the bounding box is not a single pixel
                        if bounding_box.size() > 1 {
                            if self.check_and_cache(src_fb, coordinate, row_start, CACHE_BLOCK_HEIGHT)? {
                                 row_start += CACHE_BLOCK_HEIGHT;
                                 continue;
                            }
                        };
                        bounding_box.blend_buffers(
                            src_fb,
                            dest_fb,
                            coordinate,
                            row_start,
                            CACHE_BLOCK_HEIGHT,
                        )?;
                        row_start += CACHE_BLOCK_HEIGHT;
                    } 
                }
            }
        }

        Ok(())
    }
}

/// Gets the hash of a cache block
fn hash<T: Hash>(block: T) -> u64 {
    let builder = DefaultHashBuilder::default();
    let mut hasher = builder.build_hasher();
    block.hash(&mut hasher);
    hasher.finish()
}