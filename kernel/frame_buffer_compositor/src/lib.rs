//! This crate defines a framebuffer compositor.
//! A framebuffer compositor composites a sequence of framebuffers and display them in the final framebuffer.
//! The coordinate of a frame buffer represents its origin (top-left point).
//!
//! # Cache
//! The compositor caches framebuffer blocks for better performance. 
//!
//! First, it divides every incoming framebuffer into pieces. The height of every piece is a constant 16, which is the same as the height of a character. The width of a piece is the same as the width of the framebuffer it belongs to.
//!
//! Every piece contains a block. A `block` means the updated part of a piece since last display. It starts from the left side of the piece and is specified by the index of the piece and its width. The remaining part of the piece piece is blank. For example, in a terminal, every line is a piece, and the part from the beginning of the line to the right side of the text in the line is a block.
//!
//! The compositor caches a list of displayed pieces and the width of the block in it. We use `piece` over `block` in cache because a piece consists of a continuous array so that the compositor can store its hash rather than all the pixels of a block. If an incoming `FrameBufferBlocks` carries a list of updated blocks, the compositor compares every piece and its block width with a cached one:
//! * If the two pieces are identical, ignore it.
//! * If a new *block* overlaps with an existing one, display the block and clear the remaining part of the piece till the right side of the block in the cached piece.
//! * If the two blocks are of the same location, remove the cached piece after the step above. We do not need to make sure the new block is larger than the cached one because the extra part is already cleared in the step above.
//!
//! If `FrameBufferBlocks` is `None`, the compositor will handle all of its blocks which occupy the whole pieces.
//!
//! The compositor minimizes the updated parts of a framebuffer and clears the blank parts. Even if the cache is lost or the updated blocks information is `None`, it guarantees the result is the same.

#![no_std]
#![feature(const_vec_new)]

extern crate alloc;
extern crate compositor;
extern crate frame_buffer;
extern crate spin;
#[macro_use]
extern crate lazy_static;

use alloc::collections::BTreeMap;
use alloc::vec::{Vec, IntoIter};
use core::hash::{Hash, Hasher, SipHasher};
use compositor::Compositor;
use frame_buffer::{FrameBuffer, FINAL_FRAME_BUFFER, Coord};
use spin::Mutex;

const CACHE_BLOCK_HEIGHT:usize = 16;

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
    pub blocks: Option<IntoIter<(usize, usize)>>,
}

/// Metadata that describes the cached block.
struct BlockCache {
    /// The coordinate of the block where it is rendered to the final framebuffer.
    coordinate: Coord,
    /// The hash of the content in the frame buffer.
    content_hash: u64,
    width: usize,
}

impl BlockCache {
    // checks if the coordinate is within the block
    fn contains(&self, coordinate: Coord) -> bool {
        return coordinate.x >= self.coordinate.x
            && coordinate.x < self.coordinate.x + self.width as isize
            && coordinate.y >= self.coordinate.y
            && coordinate.y < self.coordinate.y + CACHE_BLOCK_HEIGHT as isize;
    }

    // checks if the cached block overlaps with another one
    fn overlaps_with(&self, cache: &BlockCache) -> bool {
        self.contains(cache.coordinate)
            || self.contains(cache.coordinate + (cache.width as isize - 1, 0))
            || self.contains(cache.coordinate + (0, CACHE_BLOCK_HEIGHT as isize - 1))
            || self.contains(cache.coordinate + (cache.width as isize - 1, CACHE_BLOCK_HEIGHT as isize - 1))
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
            let piece_pixels = CACHE_BLOCK_HEIGHT * src_width;
            let src_buffer_len = src_width * src_height;

            // Handle all blocks if the incoming blocks parameter is None 
            let mut all_blocks = Vec::new();
            let mut blocks = match frame_buffer_blocks.blocks {
                Some(blocks) => { blocks },
                None => {
                    let block_number = (src_height - 1) / CACHE_BLOCK_HEIGHT + 1;
                    for i in 0.. block_number {
                        all_blocks.push((i, src_width))
                    }
                    all_blocks.into_iter()
                } 
            };

            while let Some((piece_index, block_width)) = blocks.next() {
                // The start pixel of the piece
                let start_index = piece_pixels * piece_index;
                let coordinate_start = coordinate + (0, (CACHE_BLOCK_HEIGHT * piece_index) as isize);
                
                // The end pixel of the piece
                let mut end_index = start_index + piece_pixels;
                let coordinate_end;
                if end_index <= src_buffer_len {
                    coordinate_end = coordinate_start + (src_width as isize, CACHE_BLOCK_HEIGHT as isize);
                } else {
                    end_index = src_buffer_len;
                    coordinate_end = coordinate + (src_width as isize, src_height as isize);
                }

                let piece = &src_fb.buffer()[start_index..end_index];
                // Skip if a piece is already cached
                if self.is_cached(&piece, &coordinate_start, src_width) {
                    continue;
                }

                // find overlapped caches
                // extend the width of the updated part to the right side of the cached block
                // remove caches of the same location
                let new_cache = BlockCache {
                    content_hash: hash(piece),
                    width: block_width,
                    coordinate: coordinate_start,
                };
                let keys: Vec<_> = self.caches.keys().cloned().collect();
                let mut update_width = new_cache.width;
                for key in keys {
                    if let Some(cache) = self.caches.get(&key) {
                        if cache.overlaps_with(&new_cache) {
                            update_width = core::cmp::max(update_width, (cache.width as isize + cache.coordinate.x - new_cache.coordinate.x) as usize);
                        }
                        if cache.coordinate == new_cache.coordinate {
                            self.caches.remove(&key);
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
                let width = core::cmp::min(
                    core::cmp::min(coordinate_end.x as usize, final_width) - final_x_start,
                    update_width,
                );
                let height = core::cmp::min(coordinate_end.y as usize, final_height) - final_y_start;

                // copy every line of the block to the final framebuffer.
                // let src_buffer = src_fb.buffer();
                for i in 0..height {
                    let dest_start = (final_y_start + i) * final_width + final_x_start;
                    let src_start = src_width * ((final_y_start + i) as isize - coordinate_start.y) as usize
                        + (final_x_start as isize - coordinate_start.x) as usize;
                    let src_end = src_start + width;
                    final_fb.buffer_copy(&(piece[src_start..src_end]), dest_start);
                }

                // insert the new cache
                self.caches.insert(coordinate_start, new_cache);

            }
        }

        Ok(())
    }

    fn is_cached(&self, block: &[u32], coordinate: &Coord, width: usize) -> bool {
        match self.caches.get(coordinate) {
            Some(cache) => {
                return cache.content_hash == hash(block) && cache.width == width;
            }
            None => return false,
        }
    }
}

// Get the hash of a cache block
fn hash<T: Hash>(block: T) -> u64 {
    let mut s = SipHasher::new();
    block.hash(&mut s);
    s.finish()
} 