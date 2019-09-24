//! This crate defines a trait of Displayable.
//! A displayable is a block of content. It can display itself in a framebuffer.

#![no_std]

extern crate alloc;
extern crate frame_buffer;
#[macro_use]
extern crate downcast_rs;

use alloc::boxed::Box;
use downcast_rs::Downcast;
use frame_buffer::FrameBuffer;

/// The displayable trait.
pub trait Displayable: Downcast + Send {
    /// Display in a framebuffer
    /// # Arguments
    /// * `(x, y)`: the position to display in the framebuffer.
    /// * `framebuffer`: the framebuffer to display in.
    fn display(
        &mut self,
        x: usize,
        y: usize,
        framebuffer: &mut dyn FrameBuffer,
    ) -> Result<(), &'static str>;

    /// Resize the displayable area.
    fn resize(&mut self, width: usize, height: usize);

    /// Gets the size of the text area.
    fn get_size(&self) -> (usize, usize);
}
impl_downcast!(Displayable);
