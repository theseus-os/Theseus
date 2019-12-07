//! This crate defines a trait of `Compositor`  .
//! A compositor composites a list of buffers to a single buffer.
//! The coordinate of a frame buffer represents the location of its origin (top-left point).

#![no_std]

extern crate alloc;
extern crate frame_buffer;
extern crate hashbrown;

use core::hash::{Hash, Hasher, BuildHasher};
use hashbrown::hash_map::{DefaultHashBuilder};
use frame_buffer::{Coord, FrameBuffer};
use alloc::collections::BTreeMap;
use alloc::boxed::Box;
use alloc::vec::Vec;

/// The height of a cache block of the compositor
pub const CACHE_BLOCK_HEIGHT:usize = 16;

/// The compositor trait.
/// A compositor composites a list of buffers to a single buffer. It caches the information of incoming buffers for better performance.
/// `T` is the type of cache used in this framebuffer.
pub trait Compositor<T: Mixer> {
    /// Composites the buffers in the bufferlist.
    ///
    /// # Arguments
    ///
    /// * `bufferlist` - A list of information about the buffers to be composited. The list is of generic type so that we can implement various compositor with different information. `U` specifices the type of item to update in compositing. It can be a rectangle block or a point.
    fn composite(
        &mut self,
        bufferlist: &[FrameBufferUpdates<'_, T>],
    ) -> Result<(), &'static str>;
}


/// The framebuffers to be composited together with the information of their updated blocks.
pub struct FrameBufferUpdates<'a, T: Mixer> {
    /// The framebuffer to be composited.
    pub framebuffer: &'a dyn FrameBuffer,
    /// The coordinate of the framebuffer where it is rendered to the final framebuffer.
    pub coordinate: Coord,
    /// The updated blocks of the framebuffer. If `blocks` is `None`, the compositor would handle all the blocks of the framebuffer.
    pub updates: Option<&'a [T]>,
}

impl Block {
    /// Creates a new block
    pub fn new(index: usize, start: usize, width: usize) -> Block {
        Block {
            index: index,
            start: start,
            width: width,
        }
    }
}

/// Metadata that describes the cached block.
pub struct BlockCache {
    /// The coordinate of the block where it is rendered to the final framebuffer.
    coordinate: Coord,
    /// The hash of the content in the frame buffer.
    content_hash: u64,
    /// The width of the block
    width: usize,
    /// The block which is cached
    block: Block,
}

impl BlockCache {
    /// Checks if a block cache overlaps with another one
    fn overlaps_with(&self, cache: &BlockCache) -> bool {
        self.contains_corner(cache) || cache.contains_corner(self)
    }

    /// checks if the coordinate is within the block
    fn contains(&self, coordinate: Coord) -> bool {
        return coordinate.x >= self.coordinate.x
            && coordinate.x < self.coordinate.x + self.width as isize
            && coordinate.y >= self.coordinate.y
            && coordinate.y < self.coordinate.y + CACHE_BLOCK_HEIGHT as isize;
    }

    /// checks if this block contains any of the four corners of `cache`.
    fn contains_corner(&self, cache: &BlockCache) -> bool {
        self.contains(cache.coordinate)
            || self.contains(cache.coordinate + (cache.width as isize - 1, 0))
            || self.contains(cache.coordinate + (0, CACHE_BLOCK_HEIGHT as isize - 1))
            || self.contains(cache.coordinate + (cache.width as isize - 1, CACHE_BLOCK_HEIGHT as isize - 1))
    }
}

/// A mixer is an item that can be mixed with the final framebuffer. A compositor can mix a list of shaped items with the final framebuffer rather than mix the whole framebuffer for better performance.
pub trait Mixer {
    /// Mix the item in the `src_fb` framebuffer with the final framebuffer. `src_coord` is the position of the source framebuffer relative to the top-left of the screen and `cache` is the cache of the compositor.
    fn mix_with(
        &self, 
        src_fb: &dyn FrameBuffer, 
        final_fb: &mut Box<dyn FrameBuffer + Send>, 
        src_coord: Coord,        
        cache: &mut BTreeMap<Coord, BlockCache>
    ) -> Result<(), &'static str>;
}

pub struct Block {
    /// The index of the block in a framebuffer
    index: usize,
    /// The left bound of the updated area
    start: usize,
    /// The width of the 
    width: usize,
}


impl Mixer for Block {
    fn mix_with(
        &self, 
        src_fb: &dyn FrameBuffer, 
        final_fb: &mut Box<dyn FrameBuffer + Send>, 
        src_coord: Coord,
        caches: &mut BTreeMap<Coord, BlockCache>
    ) -> Result<(), &'static str> {
        let (final_width, final_height) = final_fb.get_size();
        let (src_width, src_height) = src_fb.get_size();
        let src_buffer_len = src_width * src_height;
        let block_pixels = CACHE_BLOCK_HEIGHT * src_width;

        // The start pixel of the block
        let start_index = block_pixels * self.index;
        let coordinate_start = src_coord + (0, (CACHE_BLOCK_HEIGHT * self.index) as isize);

        // The end pixel of the block
        let end_index = start_index + block_pixels;
        
        let block_content = &src_fb.buffer()[start_index..core::cmp::min(end_index, src_buffer_len)];
        // Skip if a block is already cached
        if is_in_cache(&block_content, &coordinate_start, caches) {
            return Ok(());
        }

        // find overlapped caches
        // extend the width of the updated part to the right side of the cached block content
        // remove caches of the same location
        let new_cache = BlockCache {
            content_hash: hash(block_content),
            coordinate: coordinate_start,
            width: src_width,
            block: Block {
                index: 0,
                start: self.start,
                width: self.width,
            }
        };
        let keys: Vec<_> = caches.keys().cloned().collect();
        for key in keys {
            if let Some(cache) = caches.get_mut(&key) {
                if cache.overlaps_with(&new_cache) {
                    if cache.coordinate == new_cache.coordinate  && cache.width == new_cache.width {
                        caches.remove(&key);
                    } else {
                        cache.content_hash = 0;
                    }
                }
            };
        }

        let coordinate_end;
        if end_index <= src_buffer_len {
            coordinate_end = coordinate_start + (src_width as isize, CACHE_BLOCK_HEIGHT as isize);
        } else {
            // end_index = src_buffer_len;
            coordinate_end = src_coord + (src_width as isize, src_height as isize);
        }

        // skip if the block is not in the screen
        if coordinate_end.x < 0
            || coordinate_start.x > final_width as isize
            || coordinate_end.y < 0
            || coordinate_start.y > final_height as isize
        {
            return Ok(());
        }

        let final_x_start = core::cmp::max(0, coordinate_start.x) as usize;
        let final_y_start = core::cmp::max(0, coordinate_start.y) as usize;

        // just draw the part which is within the final buffer
        // Wenqiu: TODO Optimize Later
        let width = core::cmp::min(
            core::cmp::min(coordinate_end.x as usize, final_width) - final_x_start,
            self.width + self.start,
        ) - self.start;
        let height = core::cmp::min(coordinate_end.y as usize, final_height) - final_y_start;

        // copy every line of the block to the final framebuffer.
        // let src_buffer = src_fb.buffer();
        for i in 0..height {
            let dest_start = (final_y_start + i) * final_width + final_x_start + self.start;
            let src_start = src_width * ((final_y_start + i) as isize - coordinate_start.y) as usize
                + (final_x_start as isize - coordinate_start.x) as usize + self.start;
            let src_end = src_start + width;
            final_fb.composite_buffer(&(block_content[src_start..src_end]), dest_start);
        }

        // insert the new cache
        caches.insert(coordinate_start, new_cache);

        Ok(())
    }
}

impl Mixer for Coord {
    fn mix_with(
        &self, 
        src_fb: &dyn FrameBuffer,
        final_fb: &mut Box<dyn FrameBuffer + Send>, 
        src_coord: Coord,        
        _caches: &mut BTreeMap<Coord, BlockCache>
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

/// Get the hash of a cache block
fn hash<T: Hash>(block: T) -> u64 {
    let builder = DefaultHashBuilder::default();
    let mut hasher = builder.build_hasher();
    block.hash(&mut hasher);
    hasher.finish()
}

/// Checks if a coordinate is in the cache list.
fn is_in_cache(block: &[u32], coordinate: &Coord, caches: &BTreeMap<Coord, BlockCache>) -> bool {
    match caches.get(coordinate) {
        Some(cache) => {
            // The same hash means the array of two blocks are the same. Since all blocks are of the same height, two blocks of the same array must share the same width. And if their contents are the same, their content_width must be the same, too.
            return cache.content_hash == hash(block)
        }
        None => return false,
    }
}