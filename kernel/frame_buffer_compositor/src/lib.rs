//! This crate is a frame buffer manager. 
//! * It defines a FrameBuffer structure and creates new framebuffers for applications
//! * It defines a compositor and owns a final framebuffer which is mapped to the physical framebuffer. The compositor will composite a sequence of framebuffers and display them in the final framebuffer

#![no_std]

extern crate alloc;
extern crate frame_buffer;
extern crate compositor;
#[macro_use] extern crate log;

use alloc::vec::Vec;
use frame_buffer::{FrameBuffer, FINAL_FRAME_BUFFER};
use compositor::Compositor;

pub type Pixel = u32;

/// The framebuffer compositor structure. It will hold the cache of updated framebuffers for better performance.
/// Only framebuffers that have not changed will be redisplayed in the final framebuffer 
pub struct FrameCompositor {
    //Cache of updated framebuffers
}

impl Compositor<FrameBuffer> for FrameCompositor {
    /// compose a list of framebuffers to the final framebuffer. Every item in the list is a reference to a framebuffer with its position
    fn compose(bufferlist: Vec<(&FrameBuffer, i32, i32)>) -> Result<(), &'static str> {
        let mut final_fb = FINAL_FRAME_BUFFER.try().ok_or("FrameCompositor fails to get the final frame buffer")?.lock();
        let (final_width, final_height) = final_fb.get_size();        
        let final_buffer = final_fb.buffer_mut();
        // Check if the virtul frame buffer is in the mapped frame list
        for (src_fb, offset_x, offset_y) in bufferlist {
            let (src_width, src_height) = src_fb.get_size();

            let final_x_end = offset_x + src_width as i32;
            let final_y_end = offset_y + src_height as i32;

            // skip if the framebuffer is not in the screen
            if final_x_end < 0 || offset_x > final_width as i32 {
                break;
            }
            if final_y_end < 0 || offset_y > final_height as i32 {
                break;
            }

            let final_x_start = core::cmp::max(0, offset_x) as usize;
            let final_y_start = core::cmp::max(0, offset_y) as usize;

            // just composite the area that within the final buffer
            let width = core::cmp::min(final_x_end as usize, final_width) - final_x_start;
            let height = core::cmp::min(final_y_end as usize, final_height) - final_y_start;
            
            let src_buffer = src_fb.buffer();

            for i in 0..height {
                let dest_start = (final_y_start + i) * final_width + final_x_start;
                let dest_end = dest_start + width;
                let src_start = src_width * ((final_y_start + i) as i32 - offset_y) as usize + 
                    (final_x_start as i32 - offset_x) as usize;
                let src_end = src_start + width;

                final_buffer[dest_start..dest_end].copy_from_slice(
                    &(src_buffer[src_start..src_end])
                );
            }
        }

        Ok(())
    }
}