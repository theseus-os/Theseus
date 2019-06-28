//! This crate defines a trait of Displayable.
//! A displayable is a block of content. It can display itself 

#![no_std]

extern crate alloc;
extern crate frame_buffer;

use alloc::vec::Vec;
use frame_buffer::FrameBuffer;

/// The displayable trait.
/// A displayable can display itself in a framebuffer
pub trait Displayable<T> {
    fn display(&self, content: T, x:usize, y:usize, fg_color:u32, bg_color:u32, framebuffer: &mut FrameBuffer) 
        -> Result<(), &'static str>;
}