//! This crate defines a trait of Displayable.
//! A displayable is a block of content. It can display itself

#![no_std]

extern crate frame_buffer;
extern crate alloc;

use frame_buffer::FrameBuffer;
use alloc::boxed::Box;

/// The displayable trait.
pub trait Displayable<T> {
    /// to display itself in a framebuffer
    /// # Arguments
    /// * `content`: the content to be displayed.
    /// * `(x, y)`: the position to display in the framebuffer.
    /// * `fg_color`: the foreground color of the content to be displayed.
    /// * `bg_color`: the background color of the displayable.
    /// * `framebuffer`: the framebuffer to display in.
    fn display(
        &mut self,
        content: T,
        x: usize,
        y: usize,
        fg_color: u32,
        bg_color: u32,
        framebuffer: &mut FrameBuffer,
    ) -> Result<(), &'static str>;
}
