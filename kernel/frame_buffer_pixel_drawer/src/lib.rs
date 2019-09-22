//! This crate contains a basic draw function to draw a pixel in a framebuffer.
//! framebuffer drawer and printer invoke this pixel drawerto draw graphs in a framebuffer.

#![no_std]

extern crate frame_buffer_2d;
extern crate memory;
extern crate owning_ref;
extern crate frame_buffer;

use frame_buffer_2d::{FrameBufferRGB};
use frame_buffer::Pixel;
use memory::MappedPages;
use owning_ref::BoxRefMut;

// An RGB color is represented by a 24-bit integer
const COLOR_BITS: u32 = 24;

// /// draw a 3d pixel to a framebuffer
// pub fn draw_pixel_3d(framebuffer: &mut FrameBufferRGB, x: usize, y: usize, z: u8, color: Pixel) {
//     let index = framebuffer.index(x, y);
//     let buffer = framebuffer.buffer_mut();
//     write_to_3d(buffer, index, color + (z as u32) << COLOR_BITS);
// }

// /// write a 3d pixel to the index_th pixel of a buffer
// pub fn write_to_3d(buffer: &mut BoxRefMut<MappedPages, [Pixel]>, index: usize, pixel_3d: Pixel) {
//     if (buffer[index] >> COLOR_BITS) <= pixel_3d >> COLOR_BITS {
//         buffer[index] = pixel_3d;
//     }
// }
