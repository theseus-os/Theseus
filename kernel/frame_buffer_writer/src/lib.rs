//! This crate contains a series of basic draw functions to draw graphs in a framebuffer
//! Displayables invoke these basic functions to draw more compilicated graphs in a framebuffer
//! A framebuffer should be passed to the framebuffer compositor to display on the screen

#![no_std]

extern crate frame_buffer;
use frame_buffer::{FrameBuffer, Pixel};

// An  RGB color is represented by a 24-bit integer
const COLOR_BITS:u32 = 24;

// write a pixel to a framebuffer directly
pub fn write_to(framebuffer:&mut FrameBuffer, x:usize, y:usize, color:Pixel) {
    let index = framebuffer.index(x, y);
    framebuffer.buffer()[index] = color;
}

// write a 3d pizel to a framebuffer
pub fn write_to_3d(framebuffer:&mut FrameBuffer, x:usize, y:usize, z:u8, color:Pixel) {
    let index = framebuffer.index(x, y);
    let buffer = framebuffer.buffer();
    if (buffer[index] >> COLOR_BITS) <= z as u32 {
        buffer[index] = color;
    }
}