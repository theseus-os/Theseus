//! This crate defines a trait of Displayable.
//! A displayable is a block of content. It can display itself

#![no_std]

extern crate frame_buffer;
extern crate alloc;
#[macro_use]
extern crate downcast_rs;

use downcast_rs::Downcast;
use frame_buffer::FrameBuffer;
use alloc::boxed::Box;

/// The displayable trait.
pub trait Displayable: Downcast + Send {
    /// to display itself in a framebuffer
    /// # Arguments
    /// * `content`: the content to be displayed.
    /// * `(x, y)`: the position to display in the framebuffer.
    /// * `fg_color`: the foreground color of the content to be displayed.
    /// * `bg_color`: the background color of the displayable.
    /// * `framebuffer`: the framebuffer to display in.
    fn display(
        &mut self,
        x: usize,
        y: usize,
        fg_color: u32,
        bg_color: u32,
        framebuffer: &mut FrameBuffer,
    ) -> Result<(), &'static str>;

    /// resize the displayable area
    fn resize(&mut self, width: usize, height: usize);

        /// Gets the size of the text area
    fn get_size(&self) -> (usize, usize);
}
impl_downcast!(Displayable);
