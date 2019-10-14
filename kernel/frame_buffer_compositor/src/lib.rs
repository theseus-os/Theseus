//! This crate defines a framebuffer compositor.
//! A framebuffer compositor will composite a sequence of framebuffers and display them in the final framebuffer.
//! The coordinate of a frame buffer represents its origin (top-left point).

#![no_std]
#![feature(const_vec_new)]

extern crate alloc;
extern crate compositor;
extern crate frame_buffer;
extern crate spin;
#[macro_use]
extern crate log;
#[macro_use]
extern crate lazy_static;

use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use core::hash::{Hash, Hasher, SipHasher};
use compositor::Compositor;
use frame_buffer::{FrameBuffer, FINAL_FRAME_BUFFER, Coord};
use spin::Mutex;

const CACHE_BLOCK_HEIGHT:usize = 16;

lazy_static! {
    /// The instance of the frame buffer compositor.
    pub static ref FRAME_COMPOSITOR: Mutex<FrameCompositor> = Mutex::new(
        FrameCompositor{
            cache:BTreeMap::new()
        }
    );
}

/// The framebuffer compositor structure.
/// It caches framebuffers as soft states for better performance.
/// Framebuffers which haven't updated since last compositing will be ignored.
pub struct FrameCompositor {
    // Cache of updated framebuffers
    cache: BTreeMap<Coord, FrameBufferCache>,
}

/// Metadata that describes the framebuffer.
struct FrameBufferCache {
    /// The coordinate of the framebuffer where it is rendered to the final framebuffer
    coordinate: Coord,
    /// The hash of the content in the frame buffer.
    content_hash: u64,
    width: usize,
}

impl FrameBufferCache {
    // checks if the coordinate is within the framebuffer
    fn contains(&self, coordinate: Coord) -> bool {
        return coordinate.x >= self.coordinate.x
            && coordinate.x < self.coordinate.x + self.width as isize
            && coordinate.y >= self.coordinate.y
            && coordinate.y < self.coordinate.y + CACHE_BLOCK_HEIGHT as isize;
    }

    // checks if the cached framebuffer overlaps with another one
    fn overlaps_with(&self, cache: &FrameBufferCache) -> bool {
        self.contains(cache.coordinate)
            || self.contains(cache.coordinate + (cache.width as isize, 0))
            || self.contains(cache.coordinate + (0, CACHE_BLOCK_HEIGHT as isize))
            || self.contains(cache.coordinate + (cache.width as isize, CACHE_BLOCK_HEIGHT as isize))
    }
}

impl Compositor for FrameCompositor {
    fn composite(
        &mut self,
        bufferlist: Vec<(&dyn FrameBuffer, Coord, Option<(usize, usize)>)>,
    ) -> Result<(), &'static str> {
        let mut final_fb = FINAL_FRAME_BUFFER
            .try()
            .ok_or("FrameCompositor fails to get the final frame buffer")?
            .lock();
        let (final_width, final_height) = final_fb.get_size();

        for (src_fb, coordinate, block_range) in bufferlist {
            // Divide the framebuffer into 16 pixel tall blocks.
            let (src_width, src_height) = src_fb.get_size();
            let block_pixels = CACHE_BLOCK_HEIGHT * src_width;
            let (block_start, block_end) = match block_range {
                Some(b) => { b },
                None => { (0, (src_height - 1) / CACHE_BLOCK_HEIGHT + 1) }
            };
            let src_buffer_len = src_width * src_height;
                
            //trace!("WEnqiu: {} {}", block_start, block_end);
            let mut block_index = 0;
            loop {
                if block_index >= block_end {
                    break;
                }               
                // The start pixel of the block
                let start_index = block_pixels * block_index;
                // if  start_index >= src_buffer_len {
                //     break;
                // }
                let coordinate_start = coordinate + (0, (CACHE_BLOCK_HEIGHT * block_index) as isize);
                
                // The end pixel of the block
                let mut end_index = start_index + block_pixels;
                let coordinate_end;
                if end_index <= src_buffer_len {
                    coordinate_end = coordinate_start + (src_width as isize, CACHE_BLOCK_HEIGHT as isize);
                } else {
                    end_index = src_buffer_len;
                    coordinate_end = coordinate + (src_width as isize, src_height as isize);
                }

                let block = &src_fb.buffer()[start_index..end_index];
                block_index += 1;

                // Skip if a block is already cached
                if self.is_cached(&block, &coordinate_start, src_width) {
                    continue;
                }

                // skip if the framebuffer is not in the screen
                if coordinate_end.x < 0
                    || coordinate_start.x > final_width as isize
                    || coordinate_end.y < 0
                    || coordinate_start.y > final_height as isize
                {
                    break;
                }

                let final_x_start = core::cmp::max(0, coordinate_start.x) as usize;
                let final_y_start = core::cmp::max(0, coordinate_start.y) as usize;

                // just draw the part which is within the final buffer
                let width = core::cmp::min(coordinate_end.x as usize, final_width) - final_x_start;
                let height = core::cmp::min(coordinate_end.y as usize, final_height) - final_y_start;

                // copy every line to the final framebuffer.
                // let src_buffer = src_fb.buffer();
                for i in 0..height {
                    let dest_start = (final_y_start + i) * final_width + final_x_start;
                    let src_start = src_width * ((final_y_start + i) as isize - coordinate_start.y) as usize
                        + (final_x_start as isize - coordinate_start.x) as usize;
                    let src_end = src_start + width;
                    final_fb.buffer_copy(&(block[src_start..src_end]), dest_start);
                }

                // cache the new framebuffer and remove all caches that are overlapped by it.
                let new_cache = FrameBufferCache {
                    content_hash: hash(block),
                    width: src_width,
                    coordinate: coordinate_start,
                };
                let keys: Vec<_> = self.cache.keys().cloned().collect();
                for key in keys {
                    if let Some(cache) = self.cache.get(&key) {
                        if cache.overlaps_with(&new_cache) {
                            self.cache.remove(&key);
                        }
                    };
                }

                let temp = Coord::new(0, 0);

                self.cache.insert(coordinate_start, new_cache);
            }
        }

        // for (k, v) in self.cache.iter() {
        //     trace!("({} {}) ({}, {}) {} {}", k.x, k.y, v.coordinate.x, v.coordinate.y, v.content_hash, v.width);
        // }

        // loop {
        // }
        Ok(())
    }

    fn is_cached(&self, block: &[u32], coordinate: &Coord, width: usize) -> bool {
        match self.cache.get(coordinate) {
            Some(cache) => {
                return cache.content_hash == hash(block) && cache.width == width;
            }
            None => return false,
        }
    }
}

// Copy a line of pixels from src framebuffer to the dest framebuffer in 3d mode.
// We use 3d pixel drawer because we need to compare the depth of every pixel
/*fn buffer_copy_3d(
    dest_buffer: &mut BoxRefMut<MappedPages, [Pixel]>,
    src_buffer: &BoxRefMut<MappedPages, [Pixel]>,
    dest_start: usize,
    src_start: usize,
    len: usize,
) {
    let mut dest_index = dest_start;
    let dest_end = dest_start + len;
    let mut src_index = src_start;
    loop {
        frame_buffer_pixel_drawer::write_to_3d(dest_buffer, dest_index, src_buffer[src_index]);
        dest_index += 1;
        src_index += 1;
        if dest_index == dest_end {
            break;
        }
    }
}*/

// Get the hash of a cache block
fn hash<T: Hash>(block: T) -> u64 {
    let mut s = SipHasher::new();
    block.hash(&mut s);
    s.finish()
} 