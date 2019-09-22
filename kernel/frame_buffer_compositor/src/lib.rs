//! This crate is a framebuffer compositor.
//! A framebuffer compositor will compose a sequence of framebuffers and display them in the final framebuffer

#![no_std]
#![feature(const_vec_new)]

extern crate alloc;
extern crate compositor;
extern crate frame_buffer;
extern crate frame_buffer_pixel_drawer;
extern crate memory;
extern crate owning_ref;
extern crate spin;
#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate log;

use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use compositor::Compositor;
use core::hash::{Hash, Hasher, SipHasher};
use frame_buffer::{FrameBuffer, Pixel, FINAL_FRAME_BUFFER};
use memory::MappedPages;
use owning_ref::BoxRefMut;
use spin::Mutex;

lazy_static! {
    /// The instance of frame buffer compositor.
    pub static ref FRAME_COMPOSITOR: Mutex<FrameCompositor> = Mutex::new(
        FrameCompositor{
            cache:BTreeMap::new()
        }
    );
}

/// The framebuffer compositor structure. 
/// It caches updated framebuffers for better performance.
/// Framebuffers haven't changed since last compositing will be ignored
pub struct FrameCompositor {
    //Cache of updated framebuffers
    cache: BTreeMap<u64, BufferCache>,
}

struct BufferCache {
    x: i32,
    y: i32,
    width: usize,
    height: usize,
}

impl BufferCache {
    // check if the pixel is within the window
    fn check_in_area(&self, x:i32, y:i32) -> bool {
        return x >= self.x && x <= self.x + self.width as i32
                && y >= self.y && y <= self.y + self.height as i32;
    }

    fn overlap(&self, cache: &BufferCache) -> bool {
        self.check_in_area(cache.x, cache.y) ||
        self.check_in_area(cache.x + cache.width as i32, cache.y) ||
        self.check_in_area(cache.x, cache.y + cache.height as i32) ||
        self.check_in_area(cache.x + cache.width as i32, cache.y + cache.height as i32)
    }
}

impl Compositor for FrameCompositor {
    // compose a list of framebuffers to the final framebuffer. Every item in the list is a reference to a framebuffer with its position
    fn compose(&mut self, bufferlist: Vec<(&FrameBuffer, i32, i32)>) -> Result<(), &'static str> {
        let mut final_fb = FINAL_FRAME_BUFFER
            .try()
            .ok_or("FrameCompositor fails to get the final frame buffer")?
            .lock();
        let (final_width, final_height) = final_fb.get_size();
        let final_buffer = final_fb.buffer_mut();

        for (src_fb, offset_x, offset_y) in bufferlist {
            if self.cached(src_fb, offset_x, offset_y) {
                continue;
            }
            let (src_width, src_height) = src_fb.get_size();

            let final_x_end = offset_x + src_width as i32;
            let final_y_end = offset_y + src_height as i32;

            // skip if the framebuffer is not in the screen
            if final_x_end < 0
                || offset_x > final_width as i32
                || final_y_end < 0
                || offset_y > final_height as i32
            {
                break;
            }

            let final_x_start = core::cmp::max(0, offset_x) as usize;
            let final_y_start = core::cmp::max(0, offset_y) as usize;

            // just compose the part which is within the final buffer
            let width = core::cmp::min(final_x_end as usize, final_width) - final_x_start;
            let height = core::cmp::min(final_y_end as usize, final_height) - final_y_start;

            let src_buffer = src_fb.buffer();

            for i in 0..height {
                let dest_start = (final_y_start + i) * final_width + final_x_start;
                let src_start = src_width * ((final_y_start + i) as i32 - offset_y) as usize
                    + (final_x_start as i32 - offset_x) as usize;
                buffer_copy(final_buffer, src_buffer, dest_start, src_start, width);
            }

            let new_cache = BufferCache {
                x: offset_x,
                y: offset_y,
                width: src_width,
                height: src_height
            };

            let keys:Vec<_> = self.cache.keys().cloned().collect();
            for key in keys {
                match self.cache.get(&key){
                    Some(cache) => {
                        if cache.overlap(&new_cache) {
                            self.cache.remove(&key);
                        }
                    },
                    None => {}
                };
            }

            self.cache.insert(hash(&src_fb), new_cache);
        }

        Ok(())
    }

    // Check if a framebuffer has already cached since last update
    fn cached(&self, frame_buffer: &FrameBuffer, x: i32, y: i32) -> bool {
        match self.cache.get(&hash(frame_buffer)) {
            Some(cache) => {
                if cache.x == x && cache.y == y {
                    return true;
                } else {
                    return false;
                }
            }
            None => return false,
        }
    }
}


// Copy an arrary of pixels from src framebuffer to the dest framebuffer.
// We use memory copy instead of pixel drawer for better performance
fn buffer_copy(
    dest_buffer: &mut BoxRefMut<MappedPages, [Pixel]>,
    src_buffer: &BoxRefMut<MappedPages, [Pixel]>,
    dest_start: usize,
    src_start: usize,
    len: usize,
) {
    let dest_end = dest_start + len;
    let src_end = src_start + len;
    dest_buffer[dest_start..dest_end].copy_from_slice(&(src_buffer[src_start..src_end]));
}

// Copy a line of pixels from src framebuffer to the dest framebuffer in 3d mode.
// We use 3d pixel drawer because we need to compare the depth of every pixel
fn buffer_copy_3d(
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
}

// Compute the hash of a framebuffer
fn hash(t: &FrameBuffer) -> u64 {
    let mut s = SipHasher::new();
    t.hash(&mut s);
    s.finish()
}
