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
//! The compositor caches a list of displayed blocks and their updated area. If an incoming `FrameBufferBlocks` carries a list of updated blocks, the compositor compares every block with a cached one:
//! * If the two blocks are identical, ignore it.
//! * If a new block overlaps with an existing one, display the content and caches it.
//! * Then we set the hash of the old cache as 0. We do not remove it because we should keep its content location and when another block arrives, their overlapped parts will be cleared. We set its content as 0 so that the compositor will redraw it if the same block arrives.
//!
//! If `FrameBufferBlocks` is `None`, the compositor will handle all of its blocks.
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
use alloc::vec::{Vec, IntoIter};
use core::hash::{Hash, Hasher, BuildHasher};
use hashbrown::hash_map::{DefaultHashBuilder};
use compositor::Compositor;
use frame_buffer::{FrameBuffer, FINAL_FRAME_BUFFER, Coord, RectArea};
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

/// The framebuffer compositor structure.
/// It caches framebuffer blocks since last update as soft states for better performance.
pub struct FrameCompositor {
    // Cache of updated framebuffers
    caches: BTreeMap<Coord, BlockCache>,
}

/// The framebuffers to be composited together with the information of their updated blocks.
pub struct FrameBufferBlocks<'a> {
    /// The framebuffer to be composited.
    pub framebuffer: &'a dyn FrameBuffer,
    /// The coordinate of the framebuffer where it is rendered to the final framebuffer.
    pub coordinate: Coord,
    /// The updated blocks of the framebuffer. If `blocks` is `None`, the compositor would handle all the blocks of the framebuffer.
    pub blocks: Option<IntoIter<Block>>,
}

/// Metadata that describes the cached block.
struct BlockCache {
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
    // checks if the coordinate is within the block
    fn contains(&self, coordinate: Coord) -> bool {
        return coordinate.x >= self.coordinate.x
            && coordinate.x < self.coordinate.x + self.width as isize
            && coordinate.y >= self.coordinate.y
            && coordinate.y < self.coordinate.y + CACHE_BLOCK_HEIGHT as isize;
    }

    // checks if this block cotains any of the four corners of `cache`.
    fn contains_corner(&self, cache: &BlockCache) -> bool {
        self.contains(cache.coordinate)
            || self.contains(cache.coordinate + (cache.width as isize - 1, 0))
            || self.contains(cache.coordinate + (0, CACHE_BLOCK_HEIGHT as isize - 1))
            || self.contains(cache.coordinate + (cache.width as isize - 1, CACHE_BLOCK_HEIGHT as isize - 1))
    }

    // checks if the block overlaps with another one.
    fn overlaps_with(&self, cache: &BlockCache) -> bool {
        self.contains_corner(cache) || cache.contains_corner(self)
    }
}

/// A block for cache
pub struct Block {
    /// The index of the block in a framebuffer
    index: usize,
    /// The left bound of the block
    start: usize,
    /// The width of the block
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
}

impl Compositor<FrameBufferBlocks<'_>> for FrameCompositor {
    fn composite(
        &mut self,
        mut bufferlist: IntoIter<FrameBufferBlocks>,
    ) -> Result<(), &'static str> {
        let mut final_fb = FINAL_FRAME_BUFFER
            .try()
            .ok_or("FrameCompositor fails to get the final frame buffer")?
            .lock();
        let (final_width, final_height) = final_fb.get_size();

        while let Some(frame_buffer_blocks) = bufferlist.next() {
            let src_fb = frame_buffer_blocks.framebuffer;
            let coordinate = frame_buffer_blocks.coordinate;
            let (src_width, src_height) = src_fb.get_size();
            let block_pixels = CACHE_BLOCK_HEIGHT * src_width;
            let src_buffer_len = src_width * src_height;

            // Handle all blocks if the updated blocks parameter is None 
            let mut all_blocks = Vec::new();
            let mut blocks = match frame_buffer_blocks.blocks {
                Some(blocks) => { blocks },
                None => {
                    let block_number = (src_height - 1) / CACHE_BLOCK_HEIGHT + 1;
                    for i in 0.. block_number {
                        all_blocks.push(
                            Block {
                                index: i, 
                                start: 0, 
                                width: src_width
                            }
                        )
                    }
                    all_blocks.into_iter()
                } 
            };

            while let Some(block) = blocks.next() {

                // The start pixel of the block
                let start_index = block_pixels * block.index;
                let coordinate_start = coordinate + (0, (CACHE_BLOCK_HEIGHT * block.index) as isize);
                
                // The end pixel of the block
                let mut end_index = start_index + block_pixels;
                let coordinate_end;
                if end_index <= src_buffer_len {
                    coordinate_end = coordinate_start + (src_width as isize, CACHE_BLOCK_HEIGHT as isize);
                } else {
                    end_index = src_buffer_len;
                    coordinate_end = coordinate + (src_width as isize, src_height as isize);
                }

                let block_content = &src_fb.buffer()[start_index..end_index];
                // Skip if a block is already cached
                if self.is_cached(&block_content, &coordinate_start) {
                    continue;
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
                        start: block.start,
                        width: block.width,
                    }
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

                // skip if the block is not in the screen
                if coordinate_end.x < 0
                    || coordinate_start.x > final_width as isize
                    || coordinate_end.y < 0
                    || coordinate_start.y > final_height as isize
                {
                    continue;
                }

                let final_x_start = core::cmp::max(0, coordinate_start.x) as usize;
                let final_y_start = core::cmp::max(0, coordinate_start.y) as usize;

                // just draw the part which is within the final buffer
                // Wenqiu: TODO Optimize Later
                let width = core::cmp::min(
                    core::cmp::min(coordinate_end.x as usize, final_width) - final_x_start,
                    new_cache.block.width + new_cache.block.start,
                ) - new_cache.block.start;
                let height = core::cmp::min(coordinate_end.y as usize, final_height) - final_y_start;

                // copy every line of the block to the final framebuffer.
                // let src_buffer = src_fb.buffer();
                for i in 0..height {
                    let dest_start = (final_y_start + i) * final_width + final_x_start + new_cache.block.start;
                    let src_start = src_width * ((final_y_start + i) as isize - coordinate_start.y) as usize
                        + (final_x_start as isize - coordinate_start.x) as usize + new_cache.block.start;
                    let src_end = src_start + width;
                    final_fb.buffer_copy(&(block_content[src_start..src_end]), dest_start);
                }

                // insert the new cache
                self.caches.insert(coordinate_start, new_cache);

            }
        }

        Ok(())
    }

    fn composite_pixels(&mut self, mut bufferlist: IntoIter<FrameBufferBlocks>, abs_coords: &[Coord]) -> Result<(), &'static str> {
        let mut final_fb = FINAL_FRAME_BUFFER
            .try()
            .ok_or("FrameCompositor fails to get the final frame buffer")?
            .lock();
        while let Some(frame_buffer_blocks) = bufferlist.next() {
            for i in 0..abs_coords.len() {
                let coordinate = abs_coords[i];
                let relative_coord = coordinate - frame_buffer_blocks.coordinate;
                if frame_buffer_blocks.framebuffer.contains(relative_coord) {
                    let pixel = frame_buffer_blocks.framebuffer.get_pixel(relative_coord)?;
                    final_fb.draw_pixel(coordinate, pixel);
                }
            }
        }

        Ok(())
    }
    
    fn is_cached(&self, block: &[u32], coordinate: &Coord) -> bool {
        match self.caches.get(coordinate) {
            Some(cache) => {
                // The same hash means the array of two blocks are the same. Since all blocks are of the same height, two blocks of the same array must share the same width. And if their contents are the same, their content_width must be the same, too.
                return cache.content_hash == hash(block)
            }
            None => return false,
        }
    }
}

/// Compute a list of cache blocks which represent the updated area. A caller can get the block list and pass them to the compositor for better performance. 
/// 
/// # Arguments
/// * `framebuffer`: the framebuffer to composite.
/// * `area`: the updated area in this framebuffer.
pub fn get_blocks(framebuffer: &dyn FrameBuffer, area: &mut RectArea) -> Vec<Block> {
    let mut blocks = Vec::new();
    let (width, height) = framebuffer.get_size();

    let start_x = core::cmp::max(area.start.x, 0);
    let end_x = core::cmp::min(area.end.x, width as isize);
    if start_x >= end_x {
        return blocks;
    }
    let width = (end_x - start_x) as usize;        
    
    let start_y = core::cmp::max(area.start.y, 0);
    let end_y = core::cmp::min(area.end.y, height as isize);
    if start_y >= end_y {
        return blocks;
    }


    let mut index = start_y as usize / CACHE_BLOCK_HEIGHT;
    area.start.y = core::cmp::min((index * CACHE_BLOCK_HEIGHT) as isize, area.start.y);
    loop {
        if index * CACHE_BLOCK_HEIGHT >= end_y as usize {
            break;
        }
        let block = Block::new(index, start_x as usize, width);
        blocks.push(block);
        index += 1;
    }
    area.end.y = core::cmp::max((index * CACHE_BLOCK_HEIGHT) as isize, area.end.y);

    blocks
}

// Get the hash of a cache block
fn hash<T: Hash>(block: T) -> u64 {
    let builder = DefaultHashBuilder::default();
    let mut hasher = builder.build_hasher();
    block.hash(&mut hasher);
    hasher.finish()
} 