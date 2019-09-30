//! This crate defines a trait of Displayable.
//! A displayable is a block of content. It can display itself onto a framebuffer.

#![no_std]

extern crate alloc;
extern crate frame_buffer;
#[macro_use]
extern crate downcast_rs;

use alloc::boxed::Box;
use downcast_rs::Downcast;
use frame_buffer::{FrameBuffer, AbsoluteCoord};

/// The displayable trait.
pub trait Displayable: Downcast + Send {
    /// Displays in a framebuffer.
    /// # Arguments
    /// * `coordinate`: the absolute location to display onto the framebuffer.
    /// * `framebuffer`: the framebuffer to display onto.
    fn display(
        &mut self,
        coordinate: AbsoluteCoord,
        framebuffer: &mut dyn FrameBuffer,
    ) -> Result<(), &'static str>;

    /// Resizes the displayable area.
    fn resize(&mut self, width: usize, height: usize);

    /// Gets the size of the area occupied by the displayable.
    fn get_size(&self) -> (usize, usize);
}
impl_downcast!(Displayable);
