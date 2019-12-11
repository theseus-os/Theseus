//! This crate defines a framebuffer compositor.
//! A framebuffer compositor composites a sequence of framebuffers and display them in the final framebuffer.
//! The coordinate of a frame buffer represents its origin (top-left point).
//!
//! # Cache
//! The compositor caches framebuffer blocks for better performance. 
//!
//! First, it divides every incoming framebuffer into blocks. The height of every block is a constant 16. The width of a block is the same as the width of the framebuffer it belongs to. A block is a continuous array so that we can compute its hash to compare the content of two blocks.
//!
//! The `start` and `width` parameter represents the updated area in this block.
//!
//! The compositor caches a list of displayed blocks and their updated area. If an incoming `FrameBufferUpdates` carries a list of updated blocks, the compositor compares every block with a cached one:
//! * If the two blocks are identical, ignore it.
//! * If a new block overlaps with an existing one, display the content and caches it.
//! * Then we set the hash of the old cache as 0. We do not remove it because we should keep its content location and when another block arrives, their overlapped parts will be cleared. We set its content as 0 so that the compositor will redraw it if the same block arrives.
//!
//! If `FrameBufferUpdates` is `None`, the compositor will handle all of its blocks.
//!
//! The `composite_pixles` method will update the pixels relative to the top-left of the screen. It computes the relative coordinates in every framebuffer, composites them and write the result to the screen.
//! 
//! The compositor minimizes the updated parts of a framebuffer and clears the blank parts. Even if the cache is lost or the updated blocks information is `None`, it guarantees the result is the same.

#![no_std]

extern crate alloc;
extern crate compositor;
extern crate frame_buffer;
extern crate spin;
#[macro_use]
extern crate lazy_static;
extern crate hashbrown;

use alloc::collections::BTreeMap;
use alloc::vec::{Vec};
use core::hash::{Hash, Hasher, BuildHasher};
use core::ops::DerefMut;
use hashbrown::hash_map::{DefaultHashBuilder};
use compositor::{Compositor, FrameBufferUpdates, Mixable};
use frame_buffer::{FrameBuffer, FINAL_FRAME_BUFFER, Coord, Rectangle};
use spin::Mutex;

/// The height of a cache block of the compositor
pub const CACHE_BLOCK_HEIGHT:usize = 16;

lazy_static! {
    /// The instance of the frame buffer compositor.
    pub static ref FRAME_COMPOSITOR: Mutex<FrameCompositor> = Mutex::new(
        FrameCompositor{
            caches: BTreeMap::new()
        }
    );
}

/// A block profiles a rectangle area in a framebuffer. A compositor will first divide a framebuffer into blocks and only update the ones which are not cached before.
///
/// The height of every block is a constant 16. Blocks are aligned along the y-axis and `index` indicates the order of a block in the framebuffer.
/// `start` and `width` marks the area to be updated in a block in which `start` is an x coordinate relative to the leftside of the framebuffer.
///
/// After compositing, the compositor will cache the position of an updated block and the hash of its content. In the next time, for every block in a framebuffer, the compositor will ignore it if it is alreday cached.
pub struct Block {
    /// The index of the block in a framebuffer
    index: usize,
    /// The left bound of the updated area
    start: usize,
    /// The width of the 
    width: usize,
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

    /// Turn the block into a rectangle. `width` is the width of framebuffer the block is in
    pub fn into_rectangle(self, coordinate: Coord, width: usize) -> Rectangle {
        let rect = Rectangle {
            top_left: Coord::new(
                self.start as isize, 
                (self.index * CACHE_BLOCK_HEIGHT) as isize
            ),
            bottom_right: Coord::new(
                (self.start + self.width) as isize, 
                ((self.index + 1) * CACHE_BLOCK_HEIGHT) as isize
            )
        };
        rect + coordinate
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
    fn is_cached(&self, block: &[u32], coordinate: &Coord) -> bool {
        match self.caches.get(coordinate) {
            Some(cache) => {
                // The same hash means the array of two blocks are the same. Since all blocks are of the same height, two blocks of the same array must share the same width. Therefore, the coordinate and content_hash can identify a block.
                return cache.content_hash == hash(block)
            }
            None => return false,
        }
    }

    fn check_cache_and_mix(&mut self, src_fb: &dyn FrameBuffer, final_fb: &mut dyn FrameBuffer, coordinate: Coord, index: usize, area: &Rectangle) -> Result<(), &'static str> {
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
    fn composite<'a, U: IntoIterator<Item = Rectangle> + Clone>(
        &mut self,
        bufferlist: impl IntoIterator<Item = FrameBufferUpdates<'a>>,
        updates: U
    ) -> Result<(), &'static str> {
        let mut final_fb_locked = FINAL_FRAME_BUFFER.try().ok_or("FrameCompositor fails to get the final frame buffer")?.lock();
        let final_fb = final_fb_locked.deref_mut();
        let mut update_area = updates.into_iter().next();
        for frame_buffer_updates in bufferlist.into_iter() {
            let src_fb = frame_buffer_updates.framebuffer;
            let coordinate = frame_buffer_updates.coordinate;
            match &mut update_area {
                Some(area) => {
                    let blocks = get_blocks(src_fb, coordinate, area);
                    for block in blocks {
                        self.check_cache_and_mix(src_fb, final_fb.deref_mut(), coordinate, block.index, &area)?;
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
                        let block = Block::new(i, 0, src_width);
                        self.check_cache_and_mix(src_fb, final_fb.deref_mut(), coordinate, block.index, &area)?;
                    }
                } 
            };
      
        }

        Ok(())
    }
}

impl Compositor<Coord> for FrameCompositor {
    fn composite<'a, U: IntoIterator<Item = Coord> + Clone>(
        &mut self,
        bufferlist: impl IntoIterator<Item = FrameBufferUpdates<'a>>,
        updates: U
    ) -> Result<(), &'static str> {
        let mut final_fb_locked = FINAL_FRAME_BUFFER.try().ok_or("FrameCompositor fails to get the final frame buffer")?.lock();
        let final_fb = final_fb_locked.deref_mut();

        for frame_buffer_updates in bufferlist {
            for pixel in updates.clone() {
                pixel.mix_buffers(
                    frame_buffer_updates.framebuffer,
                    final_fb.deref_mut(),
                    frame_buffer_updates.coordinate,
                )?;
            }
        }
        Ok(())
    }
}

/// Compute a list of cache blocks which represent the updated area. A caller can get the block list and pass them to the compositor for better performance. 
/// 
/// # Arguments
/// * `framebuffer`: the framebuffer to composite.
/// * `coordinate`: the coordinate of the framebuffer relative to the origin(top-left) of the screen.
/// * `area`: the updated area relative to the origin(top-left) of the screen.
pub fn get_blocks(framebuffer: &dyn FrameBuffer, coordinate: Coord, abs_area: &mut Rectangle) -> Vec<Block> {
    let mut relative_area = *abs_area - coordinate;

    let mut blocks = Vec::new();
    let (width, height) = framebuffer.get_size();

    let start_x = core::cmp::max(relative_area.top_left.x, 0);
    let end_x = core::cmp::min(relative_area.bottom_right.x, width as isize);
    if start_x >= end_x {
        return blocks;
    }
    let width = (end_x - start_x) as usize;        
    
    let start_y = core::cmp::max(relative_area.top_left.y, 0);
    let end_y = core::cmp::min(relative_area.bottom_right.y, height as isize);
    if start_y >= end_y {
        return blocks;
    }


    let mut index = start_y as usize / CACHE_BLOCK_HEIGHT;
    relative_area.top_left.y = core::cmp::min((index * CACHE_BLOCK_HEIGHT) as isize, relative_area.top_left.y);
    loop {
        if index * CACHE_BLOCK_HEIGHT >= end_y as usize {
            break;
        }
        let block = Block::new(index, start_x as usize, width);
        blocks.push(block);
        index += 1;
    }
    relative_area.bottom_right.y = core::cmp::max((index * CACHE_BLOCK_HEIGHT) as isize, relative_area.bottom_right.y);

    *abs_area = relative_area + coordinate;

    blocks
}

/// Get the hash of a cache block
pub fn hash<T: Hash>(block: T) -> u64 {
    let builder = DefaultHashBuilder::default();
    let mut hasher = builder.build_hasher();
    block.hash(&mut hasher);
    hasher.finish()
}