//! This crate defines a framebuffer compositor.
//! A framebuffer compositor composites a list of framebuffers into a single destination framebuffer.
//! The coordinate of a framebuffer represents its origin (top-left point).
//!
//! # Cache
//! The compositor caches framebuffer blocks for better performance. 
//!
//! First, it divides every incoming framebuffer into blocks. The height of every block is a constant 16 except for the last one. The width of a block is the same as the width of the framebuffer it belongs to. A block is a continuous array so that we can compute its hash to compare the content of two blocks.
//!
//! In the next step, the compositor chooses all the blocks that overlap the given bounding box and checks if each block is already cached. If the answer is no, the compositor will refresh the part of the block within the bounding box.
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
#[macro_use]
extern crate log;

use alloc::collections::BTreeMap;
use alloc::vec::{Vec};
use core::hash::{Hash, Hasher, BuildHasher};
use hashbrown::hash_map::{DefaultHashBuilder};
use compositor::{Compositor, FrameBufferUpdates, BlendableRegion};
use frame_buffer::{FrameBuffer, Pixel};
use shapes::{Coord, Rectangle};
use spin::Mutex;

/// The height of a cache block. See the definition of `BlockCache`.
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
pub struct BlockCache {
    /// The coordinate of the block where it is rendered to the destination framebuffer.
    coordinate: Coord,
    /// The hash of the content in the framebuffer.
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

    /// This function will get the `index`_th block in the framebuffer and check if it is cached.
    /// If the answer is not, it will remove the cache overlaps with the block and caches the new one. 
    /// # Arguments
    /// * `src_fb`: the updated source framebuffer.
    /// * `dest_fn`: the destination framebuffer.
    /// * `coordinate`: the position of the source framebuffer relative to the destination framebuffer.
    /// * `index`: the index of the block to be rendered. 
    ///    The framebuffer are divided into y-aligned blocks and index indicates the order of the block.
    /// * `bounding_box`: the bounding box specifying the region to update.
    fn check_and_cache<P: Pixel, B: BlendableRegion>(
        &mut self, 
        src_fb: &FrameBuffer<P>, 
        dest_fb: &mut FrameBuffer<P>, 
        coordinate: Coord, 
        index: usize, 
        bounding_box: &B,
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

            self.caches.insert(coordinate_start, new_cache);        
        Ok(())
    }

    /// This function will blend the intersection of the bounding_box with the `index_th` block in the source framebuffer to the destination. `coordinate` is the top-left point of the source framebuffer relative to top-left of the distination one. About `block` see the definition of this `frame_buffer_compositor` crate.
    fn blend<B: BlendableRegion, P: Pixel>(
        &self,
        src_fb: &FrameBuffer<P>,
        dest_fb: &mut FrameBuffer<P>,
        bounding_box: &B, 
        index: usize, 
        coordinate: Coord
    ) -> Result<(), &'static str> {
        let update_box = bounding_box.intersect_block(index, coordinate, CACHE_BLOCK_HEIGHT);

        update_box.blend_buffers(
            src_fb,
            dest_fb,
            coordinate,
        )
    }

}

impl Compositor for FrameCompositor {
    fn composite<'a, B: BlendableRegion + Clone, P: 'a + Pixel>(
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
                let block_number = (src_height - 1) / CACHE_BLOCK_HEIGHT + 1;
                let area = Rectangle {
                    top_left: coordinate,
                    bottom_right: coordinate + (src_width as isize, src_height as isize)
                };
                for i in 0.. block_number {
                    self.check_and_cache(src_fb, dest_fb, coordinate, i, &area)?;
                    self.blend(src_fb, dest_fb, &area, i, coordinate)?;
                }
            }
        } else {
            for frame_buffer_updates in src_fbs.into_iter() {
                let mut updated_blocks = Vec::new();
                for bounding_box in bounding_boxes.clone() {
                    let src_fb = frame_buffer_updates.framebuffer;
                    let coordinate = frame_buffer_updates.coordinate;
                    let blocks = bounding_box.get_block_index_iter(src_fb, coordinate, CACHE_BLOCK_HEIGHT);
                    for block in blocks {
                        // The same block is cached only once
                        if !updated_blocks.contains(&block) {
                            updated_blocks.push(block);
                            self.check_and_cache(src_fb, dest_fb, coordinate, block, &bounding_box.clone())?;
                        };
                        self.blend(src_fb, dest_fb, &bounding_box.clone(), block, coordinate)?;                        
                    } 
                }
            }
        }

        Ok(())
    }
}

// impl Compositor<Coord> for FrameCompositor {
//     fn composite<'a, U: IntoIterator<Item = Coord> + Clone, P: 'a + Pixel>(
//         &mut self,
//         src_fbs: impl IntoIterator<Item = FrameBufferUpdates<'a, P>>,
//         dest_fb: &mut FrameBuffer<P>,
//         bounding_boxes: U
//     ) -> Result<(), &'static str> {
//         let mut box_iter = bounding_boxes.clone().into_iter();
//         if box_iter.next().is_none() {
//             for frame_buffer_updates in src_fbs.into_iter() {
//                 let src_fb = frame_buffer_updates.framebuffer;
//                 let coordinate = frame_buffer_updates.coordinate;
//                 // Update the whole screen if the caller does not specify the blocks
//                 let (src_width, src_height) = frame_buffer_updates.framebuffer.get_size();
//                 let block_number = (src_height - 1) / CACHE_BLOCK_HEIGHT + 1;
//                 let area = Rectangle {
//                     top_left: coordinate,
//                     bottom_right: coordinate + (src_width as isize, src_height as isize)
//                 };
//                 for i in 0.. block_number {
//                     self.check_cache_and_blend(src_fb, dest_fb, coordinate, i, &area, true)?;
//                 }
//             }
//         } else {
//             for frame_buffer_updates in src_fbs.into_iter() {
//                 for rect in bounding_boxes.clone() {
//                     let src_fb = frame_buffer_updates.framebuffer;
//                     let coordinate = frame_buffer_updates.coordinate;
//                     let blocks = rect.get_block_index_iter(src_fb, coordinate, CACHE_BLOCK_HEIGHT);
//                     for block in blocks {
//                         self.check_cache_and_blend(src_fb, dest_fb, coordinate, block, &rect.clone(), true)?;
//                     } 
//                 }
//             }
//         }

//         Ok(())
//     }
// }

// /// Gets an iterator over the block indexes to update in the framebuffer.
// /// # Arguments
// /// * `framebuffer`: the framebuffer to composite.
// /// * `coordinate`: the coordinate of the framebuffer relative to the origin(top-left) of the screen.
// /// * `bounding_box`: the bounding box to update relative to the origin(top-left) of the screen. The returned indexes represent the blocks overlap with this area.
// pub fn get_block_index_iter<P: Pixel>(
//     framebuffer: &FrameBuffer<P>, 
//     coordinate: Coord, 
//     bounding_box: &Rectangle
// ) -> core::ops::Range<usize> {
//     let relative_area = *bounding_box - coordinate;
//     let (width, height) = framebuffer.get_size();

//     let start_x = core::cmp::max(relative_area.top_left.x, 0);
//     let end_x = core::cmp::min(relative_area.bottom_right.x, width as isize);
//     if start_x >= end_x {
//         return 0..0;
//     }
    
//     let start_y = core::cmp::max(relative_area.top_left.y, 0);
//     let end_y = core::cmp::min(relative_area.bottom_right.y, height as isize);
//     if start_y >= end_y {
//         return 0..0;
//     }
//     let start_index = start_y as usize / CACHE_BLOCK_HEIGHT;
//     let end_index = end_y as usize / CACHE_BLOCK_HEIGHT + 1;
    
//     return start_index..end_index
// }

/// Gets the hash of a cache block
fn hash<T: Hash>(block: T) -> u64 {
    let builder = DefaultHashBuilder::default();
    let mut hasher = builder.build_hasher();
    block.hash(&mut hasher);
    hasher.finish()
}