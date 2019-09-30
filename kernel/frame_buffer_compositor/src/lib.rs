//! This crate is a framebuffer compositor.
//! A framebuffer compositor will composite a sequence of framebuffers and display them in the final framebuffer.

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
use frame_buffer::{FrameBuffer, FINAL_FRAME_BUFFER, ICoord};
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
    cache: BTreeMap<u64, BufferCache>,
}

// The information of a cached framebuffer. It contains the position and size of the framebuffer.
struct BufferCache {
    coordinate: ICoord,
    width: usize,
    height: usize,
}

impl BufferCache {
    // checks if the pixel is within the framebuffer
    fn contains_coordinate(&self, coordinate: ICoord) -> bool {
        return coordinate.x >= self.coordinate.x
            && coordinate.x <= self.coordinate.x + self.width as i32
            && coordinate.y >= self.coordinate.y
            && coordinate.y <= self.coordinate.y + self.height as i32;
    }

    // checks if the cached framebuffer overlaps with another one
    fn overlaps_with(&self, cache: &BufferCache) -> bool {
        self.contains_coordinate(cache.coordinate)
            || self.contains_coordinate(cache.coordinate + (cache.width as i32, 0))
            || self.contains_coordinate(cache.coordinate + (0, cache.height as i32))
            || self.contains_coordinate(cache.coordinate + (cache.width as i32, cache.height as i32))
    }
}

impl Compositor for FrameCompositor {
    fn composite(
        &mut self,
        bufferlist: Vec<(&dyn FrameBuffer, ICoord)>,
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

            let coordinate_end = coordinate + (src_width as i32, src_height as i32);

            // skip if the framebuffer is not in the screen
            if coordinate_end.x < 0
                || coordinate.x > final_width as i32
                || coordinate_end.y < 0
                || coordinate.y > final_height as i32
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
                let src_start = src_width * ((final_y_start + i) as i32 - coordinate.y) as usize
                    + (final_x_start as i32 - coordinate.x) as usize;
                let src_end = src_start + width;
                final_fb.buffer_copy(&(src_buffer[src_start..src_end]), dest_start);
            }

            // cache the new framebuffer and remove all caches that are overlapped by it.
            let new_cache = BufferCache {
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

    fn cached(&self, frame_buffer: &dyn FrameBuffer, coordinate: ICoord) -> bool {
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
