//! This crate defines a trait of `Displayable`.
//! A displayable is a block of content. It can display itself onto a framebuffer.
//! The coordinate of a displayable represents its origin (top-left point).

#![no_std]

extern crate alloc;
extern crate frame_buffer;
#[macro_use]
extern crate downcast_rs;
extern crate shapes;

use alloc::boxed::Box;
use downcast_rs::Downcast;
use frame_buffer::{FrameBuffer, Pixel};
use shapes::{Coord, Rectangle};

/// Trait for displayables. A displayable is an item which can display itself onto a framebuffer. 
/// It is usually a composition of basic graphs and can act as a component such as a text box, a button belonging to a window, etc. 
pub trait Displayable<T: Pixel + Copy> {
    /// Displays in a framebuffer.
    /// # Arguments
    /// * `coordinate`: the coordinate within the given `framebuffer` where this displayable should render itself. The `coordinate` is relative to the top-left point `(0, 0)` of the `framebuffer`.
    /// * `framebuffer`: the framebuffer to display onto.
    ///
    /// Returns a rectangle that covers the updated area.
    fn display(
        &mut self,
        coordinate: Coord,
        framebuffer: &mut FrameBuffer<T>,
    ) -> Result<Rectangle, &'static str> ;

    /// Resizes the displayable area.
    fn resize(&mut self, width: usize, height: usize);

    /// Gets the size of the area occupied by the displayable.
    fn get_size(&self) -> (usize, usize);
}
