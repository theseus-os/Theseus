//! This crate contains a basic draw function to draw a pixel in a framebuffer
//! framebuffer drawer and printer invoke this pixel drawerto draw graphs in a framebuffer

#![no_std]

extern crate frame_buffer;
use frame_buffer::{FrameBuffer, Pixel};

// An RGB color is represented by a 24-bit integer
const COLOR_BITS: u32 = 24;

// write a pixel to a framebuffer directly
pub fn draw_pixel(framebuffer: &mut FrameBuffer, x: usize, y: usize, color: Pixel) {
    let index = framebuffer.index(x, y);
    framebuffer.buffer_mut()[index] = color;
}

// write a 3d pizel to a framebuffer
pub fn draw_pixel_3d(framebuffer: &mut FrameBuffer, x: usize, y: usize, z: u8, color: Pixel) {
    let index = framebuffer.index(x, y);
    let buffer = framebuffer.buffer_mut();
    if (buffer[index] >> COLOR_BITS) <= z as u32 {
        buffer[index] = color;
    }
}