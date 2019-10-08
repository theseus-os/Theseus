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
extern crate lazy_static;

use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use compositor::Compositor;
use frame_buffer::{FrameBuffer, FINAL_FRAME_BUFFER, Coord};
use spin::Mutex;

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
    cache: BTreeMap<u64, FrameBufferCache>,
}

/// Metadata that describes where a framebuffer was previously composited to the final framebuffer.
struct FrameBufferCache {
    /// The position at which the framebuffer was rendered, which is relative to the final framebuffer's coordinate system.
    coordinate: Coord,
    width: usize,
    height: usize,
}

impl FrameBufferCache {
    // checks if the coordinate is within the framebuffer
    fn contains(&self, coordinate: Coord) -> bool {
        return coordinate.x >= self.coordinate.x
            && coordinate.x <= self.coordinate.x + self.width as isize
            && coordinate.y >= self.coordinate.y
            && coordinate.y <= self.coordinate.y + self.height as isize;
    }

    // checks if the cached framebuffer overlaps with another one
    fn overlaps_with(&self, cache: &FrameBufferCache) -> bool {
        self.contains(cache.coordinate)
            || self.contains(cache.coordinate + (cache.width as isize, 0))
            || self.contains(cache.coordinate + (0, cache.height as isize))
            || self.contains(cache.coordinate + (cache.width as isize, cache.height as isize))
    }
}

impl Compositor for FrameCompositor {
    fn composite(
        &mut self,
        bufferlist: Vec<(&dyn FrameBuffer, Coord)>,
    ) -> Result<(), &'static str> {
        let mut final_fb = FINAL_FRAME_BUFFER
            .try()
            .ok_or("FrameCompositor fails to get the final frame buffer")?
            .lock();
        let (final_width, final_height) = final_fb.get_size();

        for (src_fb, coordinate) in bufferlist {
            // skip if already cached
            if self.cached(src_fb, coordinate) {
                continue;
            }
            let (src_width, src_height) = src_fb.get_size();

            let coordinate_end = coordinate + (src_width as isize, src_height as isize);

            // skip if the framebuffer is not in the screen
            if coordinate_end.x < 0
                || coordinate.x > final_width as isize
                || coordinate_end.y < 0
                || coordinate.y > final_height as isize
            {
                break;
            }

            let final_x_start = core::cmp::max(0, coordinate.x) as usize;
            let final_y_start = core::cmp::max(0, coordinate.y) as usize;

            // just draw the part which is within the final buffer
            let width = core::cmp::min(coordinate_end.x as usize, final_width) - final_x_start;
            let height = core::cmp::min(coordinate_end.y as usize, final_height) - final_y_start;

            // copy every line to the final framebuffer.
            let src_buffer = src_fb.buffer();
            for i in 0..height {
                let dest_start = (final_y_start + i) * final_width + final_x_start;
                let src_start = src_width * ((final_y_start + i) as isize - coordinate.y) as usize
                    + (final_x_start as isize - coordinate.x) as usize;
                let src_end = src_start + width;
                final_fb.buffer_copy(&(src_buffer[src_start..src_end]), dest_start);
            }

            // cache the new framebuffer and remove all caches that are overlapped by it.
            let new_cache = FrameBufferCache {
                coordinate: coordinate,
                width: src_width,
                height: src_height,
            };
            let keys: Vec<_> = self.cache.keys().cloned().collect();
            for key in keys {
                if let Some(cache) = self.cache.get(&key) {
                    if cache.overlaps_with(&new_cache) {
                        self.cache.remove(&key);
                    }
                };
            }
            self.cache.insert(src_fb.get_hash(), new_cache);
        }

        Ok(())
    }

    fn cached(&self, frame_buffer: &dyn FrameBuffer, coordinate: Coord) -> bool {
        match self.cache.get(&(frame_buffer.get_hash())) {
            Some(cache) => {
                return cache.coordinate == coordinate;
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
