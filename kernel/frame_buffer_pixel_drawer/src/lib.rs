//! This crate contains a basic draw function to draw a pixel in a framebuffer.
//! framebuffer drawer and printer invoke this pixel drawerto draw graphs in a framebuffer.

#![no_std]

extern crate frame_buffer;
extern crate memory;
extern crate owning_ref;

use frame_buffer::{FrameBuffer, Pixel};
use owning_ref::BoxRefMut;
use memory::{MappedPages};

// An RGB color is represented by a 24-bit integer
const COLOR_BITS: u32 = 24;

// write a pixel to a framebuffer directly
pub fn draw_pixel(framebuffer: &mut FrameBuffer, x: usize, y: usize, color: Pixel) {
    let index = framebuffer.index(x, y);
    framebuffer.buffer_mut()[index] = color;
}

// draw a 3d pixel to a framebuffer
pub fn draw_pixel_3d(framebuffer: &mut FrameBuffer, x: usize, y: usize, z: u8, color: Pixel) {
    let index = framebuffer.index(x, y);
    let buffer = framebuffer.buffer_mut();
    write_to_3d(buffer, index, color + (z as u32) << COLOR_BITS);
}

// write a 3d color to a buffer
pub fn write_to_3d(buffer: &mut BoxRefMut<MappedPages, [Pixel]>, index:usize, color_3d: Pixel) {
    if (buffer[index] >> COLOR_BITS) <= color_3d >> COLOR_BITS {
        buffer[index] = color_3d;
    }
}