//! This crate defines a trait of Displayable.
//! A displayable is a block of content. It can display itself onto a framebuffer.
//! The coordinate of a displayable represents the location of its top-left corner.

#![no_std]

extern crate alloc;
extern crate frame_buffer;
#[macro_use]
extern crate downcast_rs;

use alloc::boxed::Box;
use downcast_rs::Downcast;
use frame_buffer::{FrameBuffer, Coord};

/// The displayable trait.
pub trait Displayable: Downcast + Send {
    /// Displays in a framebuffer.
    /// # Arguments
    /// * `coordinate`: the coordinate within the given `framebuffer` where this displayable should render itself. The `coordinate` is relative to the top-left corner `(0, 0)` of the `framebuffer`.
    /// * `framebuffer`: the framebuffer to display onto.
    fn display(
        &mut self,
        coordinate: Coord,
        framebuffer: &mut dyn FrameBuffer,
    ) -> Result<(), &'static str>;

    /// Resizes the displayable area.
    fn resize(&mut self, width: usize, height: usize);

    /// Gets the size of the area occupied by the displayable.
    fn get_size(&self) -> (usize, usize);
}
impl_downcast!(Displayable);
