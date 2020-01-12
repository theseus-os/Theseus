//! This crate defines a trait of `Displayable`.
//! A displayable is a block of content. It can display itself onto a framebuffer.
//! The coordinate of a displayable represents its origin (top-left point).

#![no_std]

extern crate frame_buffer;
extern crate shapes;
extern crate color;

use frame_buffer::{Framebuffer, Pixel};
use shapes::{Coord, Rectangle};
use color::Color;

/// The `Displayable` trait is an abstraction for any object that can display itself onto a framebuffer. 
/// Examples include a text box, button, window border, etc.
pub trait Displayable {
    /// Displays in a framebuffer.
    /// # Arguments
    /// * `coordinate`: the coordinate within the given `framebuffer` where this displayable should render itself.
    ///    The `coordinate` is relative to the top-left point of the `framebuffer`.
    /// * `framebuffer`: the framebuffer to display onto.
    ///
    /// Returns a rectangle that represents the region of the framebuffer that was updated.
    fn display<P: Pixel + From<Color>>(
        &mut self,
        coordinate: Coord,
        framebuffer: &mut Framebuffer<P>,
    ) -> Result<Rectangle, &'static str>;

    /// Resizes the displayable area.
    fn resize(&mut self, width: usize, height: usize);

    /// Gets the size of the area occupied by the displayable.
    fn get_size(&self) -> (usize, usize);
}
