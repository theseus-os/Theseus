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
use alloc::boxed::Box;
use core::hash::{Hash, Hasher, BuildHasher};
use core::ops::DerefMut;
use hashbrown::hash_map::{DefaultHashBuilder};
use compositor::{Compositor, FrameBufferUpdates, Mixer, Block, BlockCache, CACHE_BLOCK_HEIGHT};
use frame_buffer::{FrameBuffer, FINAL_FRAME_BUFFER, Coord, Rectangle};
use spin::Mutex;

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

/// A block for cache. 
///
/// In order to composite a framebuffer, the compositor will first divide it into blocks. 
/// The height of every block is a constant 16. Blocks are aligned along the y-axis and `index` indicates the order of a block.
/// `start` and `width` marks the area to be updated in a block in which `start` is an x coordinate relative to the leftside of the framebuffer. It the compositor gets a framebuffer together with some blocks, it just composite the area specified by these blocks.
///
/// After compositing, the compositor will cache the updated blocks. In the next time, for every block in a framebuffer, the compositor will ignore it if it is alreday cached.

impl Compositor<Block> for FrameCompositor {
    fn composite(
        &mut self,
        bufferlist: &[FrameBufferUpdates<'_, Block>],
    ) -> Result<(), &'static str> {
        let mut final_fb = FINAL_FRAME_BUFFER
            .try()
            .ok_or("FrameCompositor fails to get the final frame buffer")?
            .lock();

        for frame_buffer_updates in bufferlist {
            let src_fb = frame_buffer_updates.framebuffer;
            let coordinate = frame_buffer_updates.coordinate;
            let (src_width, src_height) = src_fb.get_size();

            // Handle all blocks if the updated blocks parameter is None 
            if frame_buffer_updates.updates.is_none() {
                let block_number = (src_height - 1) / CACHE_BLOCK_HEIGHT + 1;
                for i in 0.. block_number {
                    let block = Block::new(i, 0, src_width);
                    block.mix_with(src_fb, final_fb.deref_mut(), coordinate, &mut self.caches)?;
                }
            } else {
                let updates = match frame_buffer_updates.updates {
                    Some(updates) => { updates },
                    None => {
                        continue;
                    } 
                };
                for item in updates {
                    item.mix_with(
                        frame_buffer_updates.framebuffer,
                        final_fb.deref_mut(),
                        frame_buffer_updates.coordinate,
                        &mut self.caches
                    )?;
                }
            }

        }

        Ok(())
    }
}

impl Compositor<Coord> for FrameCompositor {
    fn composite(
        &mut self,
        bufferlist: &[FrameBufferUpdates<'_, Coord>],
    ) -> Result<(), &'static str> {
        let mut final_fb = FINAL_FRAME_BUFFER
            .try()
            .ok_or("FrameCompositor fails to get the final frame buffer")?
            .lock();

        for frame_buffer_updates in bufferlist {
            let src_fb = frame_buffer_updates.framebuffer;
            let coordinate = frame_buffer_updates.coordinate;
            let (src_width, src_height) = src_fb.get_size();

            // Handle all blocks if the updated blocks parameter is None 
            if frame_buffer_updates.updates.is_none() {
                let block_number = (src_height - 1) / CACHE_BLOCK_HEIGHT + 1;
                for i in 0.. block_number {
                    let block = Block::new(i, 0, src_width);
                    block.mix_with(src_fb, final_fb.deref_mut(), coordinate, &mut self.caches)?;
                }
            } else {
                let updates = match frame_buffer_updates.updates {
                    Some(updates) => { updates },
                    None => {
                        continue;
                    } 
                };
                for item in updates {
                    item.mix_with(
                        frame_buffer_updates.framebuffer,
                        final_fb.deref_mut(),
                        frame_buffer_updates.coordinate,
                        &mut self.caches
                    )?;
                }
            }

        }

        Ok(())
    }
}

/// Compute a list of cache blocks which represent the updated area. A caller can get the block list and pass them to the compositor for better performance. 
/// 
/// # Arguments
/// * `framebuffer`: the framebuffer to composite.
/// * `area`: the updated area in this framebuffer.
pub fn get_blocks(framebuffer: &dyn FrameBuffer, area: &mut Rectangle) -> Vec<Block> {
    let mut blocks = Vec::new();
    let (width, height) = framebuffer.get_size();

    let start_x = core::cmp::max(area.top_left.x, 0);
    let end_x = core::cmp::min(area.bottom_right.x, width as isize);
    if start_x >= end_x {
        return blocks;
    }
    let width = (end_x - start_x) as usize;        
    
    let start_y = core::cmp::max(area.top_left.y, 0);
    let end_y = core::cmp::min(area.bottom_right.y, height as isize);
    if start_y >= end_y {
        return blocks;
    }


    let mut index = start_y as usize / CACHE_BLOCK_HEIGHT;
    area.top_left.y = core::cmp::min((index * CACHE_BLOCK_HEIGHT) as isize, area.top_left.y);
    loop {
        if index * CACHE_BLOCK_HEIGHT >= end_y as usize {
            break;
        }
        let block = Block::new(index, start_x as usize, width);
        blocks.push(block);
        index += 1;
    }
    area.bottom_right.y = core::cmp::max((index * CACHE_BLOCK_HEIGHT) as isize, area.bottom_right.y);

    blocks
}

