//! This crate defines a framebuffer compositor.
//! A framebuffer compositor composites a sequence of framebuffers and display them in the final framebuffer.
//! The coordinate of a frame buffer represents its origin (top-left point).
//!
//! # Cache
//! The compositor caches framebuffer blocks for better performance. 
//!
//! First, it divides every incoming framebuffer into blocks. The height of every block is a constant 16 except for the last one. The width of a block is the same as the width of the framebuffer it belongs to. A block is a continuous array so that we can compute its hash to compare the content of two blocks.
//!
//! In the next step, the compositor chooses all the blocks that overlap the given shape of pixels to be displayed and checks if each block is already cached. If the answer is no, the compositor will refresh the intersection of the block with the updated shape.
//!
//! Once a block is refreshed, the compositor will remove all the existing caches overlap with it and cache the new one.

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
use compositor::{Compositor, FrameBufferUpdates, Mixable};
use frame_buffer::{FrameBuffer, Pixel};
use shapes::{Coord, Rectangle};
use spin::Mutex;

/// The height of a cache block. See the definition of `BlockCache`.
pub const CACHE_BLOCK_HEIGHT:usize = 16;

lazy_static! {
    /// The instance of the frame buffer compositor.
    pub static ref FRAME_COMPOSITOR: Mutex<FrameCompositor> = Mutex::new(
        FrameCompositor{
            caches: BTreeMap::new()
        }
    );
}

/// Metadata that describes the cached block.
pub struct BlockCache {
    /// The coordinate of the block where it is rendered to the final framebuffer.
    coordinate: Coord,
    /// The hash of the content in the frame buffer.
    content_hash: u64,
    /// The width of the block
    width: usize,
}

impl BlockCache {
    /// Checks if a block cache overlaps with another one
    pub fn overlaps_with(&self, cache: &BlockCache) -> bool {
        self.contains_corner(cache) || cache.contains_corner(self)
    }

    /// checks if the coordinate is within the block
    fn contains(&self, coordinate: Coord) -> bool {
        return coordinate.x >= self.coordinate.x
            && coordinate.x < self.coordinate.x + self.width as isize
            && coordinate.y >= self.coordinate.y
            && coordinate.y < self.coordinate.y + CACHE_BLOCK_HEIGHT as isize;
    }

    /// checks if this block contains any of the four corners of another `cache`.
    fn contains_corner(&self, cache: &BlockCache) -> bool {
        self.contains(cache.coordinate)
            || self.contains(cache.coordinate + (cache.width as isize - 1, 0))
            || self.contains(cache.coordinate + (0, CACHE_BLOCK_HEIGHT as isize - 1))
            || self.contains(cache.coordinate + (cache.width as isize - 1, CACHE_BLOCK_HEIGHT as isize - 1))
    }
}

/// The framebuffer compositor structure.
/// It caches framebuffer blocks since last update as soft states for better performance.
pub struct FrameCompositor {
    // Cache of updated framebuffers
    caches: BTreeMap<Coord, BlockCache>,
}

impl FrameCompositor {
    /// Checks if a coordinate is in the cache list.
    fn is_cached<P: Pixel>(&self, block: &[P], coordinate: &Coord) -> bool {
        match self.caches.get(coordinate) {
            Some(cache) => {
                // The same hash means the array of two blocks are the same. Since all blocks are of the same height, two blocks of the same array must share the same width. Therefore, the coordinate and content_hash can identify a block.
                return cache.content_hash == hash(block)
            }
            None => return false,
        }
    }

    /// Checks if a block is already cached and update the new blocks.
    /// This function will get the `index`_th block in the framebuffer and check if it is cached. If not, it will update the intersection of the block and the update are.
    /// It then removes the cache overlaps with the block and caches the new one. 
    /// # Arguments
    /// * `src_fb`: the updated source framebuffer.
    /// * `final_fn`: the final framebuffer mapped to the screen.
    /// * `coordinate`: the position of the source framebuffer relative to the final one.
    /// * `index`: the index of the block to be rendered. The framebuffer are divided into y-aligned blocks and index indicates the order of the block.
    /// * `area`: the rectangle to be updated.
    fn check_cache_and_mix<P: Pixel>(
        &mut self, 
        src_fb: &FrameBuffer<P>, 
        final_fb: &mut FrameBuffer<P>, 
        coordinate: Coord, 
        index: usize, 
        area: &Rectangle
    ) -> Result<(), &'static str> {
        let (src_width, src_height) = src_fb.get_size();
        let src_buffer_len = src_width * src_height;
        let block_pixels = CACHE_BLOCK_HEIGHT * src_width;

        // The start pixel of the block
        let start_index = block_pixels * index;
        let coordinate_start = coordinate + (0, (CACHE_BLOCK_HEIGHT * index) as isize);

        // The end pixel of the block
        let end_index = start_index + block_pixels;
        
        let block_content = &src_fb.buffer()[start_index..core::cmp::min(end_index, src_buffer_len)];
        
        // Skip if a block is already cached
        if self.is_cached(&block_content, &coordinate_start) {
            return Ok(());
        }

        // find overlapped caches
        // extend the width of the updated part to the right side of the cached block content
        // remove caches of the same location
        let new_cache = BlockCache {
            content_hash: hash(block_content),
            coordinate: coordinate_start,
            width: src_width,
        };
        let keys: Vec<_> = self.caches.keys().cloned().collect();
        for key in keys {
            if let Some(cache) = self.caches.get_mut(&key) {
                if cache.overlaps_with(&new_cache) {
                    if cache.coordinate == new_cache.coordinate  && cache.width == new_cache.width {
                        self.caches.remove(&key);
                    } else {
                        cache.content_hash = 0;
                    }
                }
            };
        }

        let update_rect = Rectangle {
            top_left: Coord::new(
                area.top_left.x,
                core::cmp::max((index * CACHE_BLOCK_HEIGHT) as isize + coordinate.y, area.top_left.y),
            ),
            bottom_right: Coord::new(
                area.bottom_right.x,
                core::cmp::min(((index + 1) * CACHE_BLOCK_HEIGHT) as isize + coordinate.y, area.bottom_right.y)
            )
        };

        // render to the final framebuffer
        update_rect.mix_buffers(
            src_fb,
            final_fb,
            coordinate,
        )?;

        // insert the new cache
        self.caches.insert(coordinate_start, new_cache);

        Ok(())
    }

}

impl Compositor<Rectangle> for FrameCompositor {
    fn composite<'a, U: IntoIterator<Item = Rectangle> + Clone, P: 'a + Pixel>(
        &mut self,
        bufferlist: impl IntoIterator<Item = FrameBufferUpdates<'a, P>>,
        final_fb: &mut FrameBuffer<P>,
        updates: U
    ) -> Result<(), &'static str> {
        //let mut final_fb = FINAL_FRAME_BUFFER.try().ok_or("FrameCompositor fails to get the final frame buffer")?.lock();
        //let final_fb = final_fb_locked.deref_mut();
        let update_area = updates.into_iter().next();
        for frame_buffer_updates in bufferlist.into_iter() {
            let src_fb = frame_buffer_updates.framebuffer;
            let coordinate = frame_buffer_updates.coordinate;
            match &update_area {
                Some(area) => {
                    let blocks = get_block_index_iter(src_fb, coordinate, area);
                    for block in blocks {
                        self.check_cache_and_mix(src_fb, final_fb, coordinate, block, &area)?;
                    } 
                },
                None => {
                    // Update the whole screen if the caller does not specify the blocks
                    let (src_width, src_height) = frame_buffer_updates.framebuffer.get_size();
                    let block_number = (src_height - 1) / CACHE_BLOCK_HEIGHT + 1;
                    let area = Rectangle {
                        top_left: coordinate,
                        bottom_right: coordinate + (src_width as isize, src_height as isize)
                    };
                    for i in 0.. block_number {
                        self.check_cache_and_mix(src_fb, final_fb, coordinate, i, &area)?;
                    }
                } 
            };
      
        }

        Ok(())
    }
}

impl Compositor<Coord> for FrameCompositor {
    fn composite<'a, U: IntoIterator<Item = Coord> + Clone, P: 'a + Pixel>(
        &mut self,
        bufferlist: impl IntoIterator<Item = FrameBufferUpdates<'a, P>>,
        final_fb: &mut FrameBuffer<P>,
        updates: U
    ) -> Result<(), &'static str> {
        //let mut final_fb = FINAL_FRAME_BUFFER.try().ok_or("FrameCompositor fails to get the final frame buffer")?.lock();

        for frame_buffer_updates in bufferlist {
            for pixel in updates.clone() {
                pixel.mix_buffers(
                    frame_buffer_updates.framebuffer,
                    final_fb,
                    frame_buffer_updates.coordinate,
                )?;
            }
        }
        Ok(())
    }
}

/// Gets a iterator over the block indexes to be updated in the framebuffer.
/// # Arguments
/// * `framebuffer`: the framebuffer to composite.
/// * `coordinate`: the coordinate of the framebuffer relative to the origin(top-left) of the screen.
/// * `area`: the updated area relative to the origin(top-left) of the screen. The returned indexes represent the blocks overlap with this area.
pub fn get_block_index_iter<P: Pixel>(
    framebuffer: &FrameBuffer<P>, 
    coordinate: Coord, 
    abs_area: &Rectangle
) -> core::ops::Range<usize> {
    let relative_area = *abs_area - coordinate;
    let (width, height) = framebuffer.get_size();

    let start_x = core::cmp::max(relative_area.top_left.x, 0);
    let end_x = core::cmp::min(relative_area.bottom_right.x, width as isize);
    if start_x >= end_x {
        return 0..0;
    }
    
    let start_y = core::cmp::max(relative_area.top_left.y, 0);
    let end_y = core::cmp::min(relative_area.bottom_right.y, height as isize);
    if start_y >= end_y {
        return 0..0;
    }
    let start_index = start_y as usize / CACHE_BLOCK_HEIGHT;
    let end_index = end_y as usize / CACHE_BLOCK_HEIGHT + 1;
    
    return start_index..end_index
}

/// Gets the hash of a cache block
fn hash<T: Hash>(block: T) -> u64 {
    let builder = DefaultHashBuilder::default();
    let mut hasher = builder.build_hasher();
    block.hash(&mut hasher);
    hasher.finish()
}