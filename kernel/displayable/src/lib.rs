//! This crate defines a trait of `Displayable`.
//! A displayable is a block of content. It can display itself onto a framebuffer.
//! The coordinate of a displayable represents its origin (top-left point).

#![no_std]

extern crate framebuffer;
extern crate shapes;
extern crate color;

use framebuffer::{Framebuffer, Pixel};
use shapes::{Coord, Rectangle};
use color::Color;

/// The `Displayable` trait is an abstraction for any object that can display itself onto a framebuffer. 
/// Examples include a text box, button, window border, etc.
pub trait Displayable {
    /// Displays this `Displayable`'s content in the given framebuffer.
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

    /// Resizes the displayable area, but does not automatically refresh its display.
    fn set_size(&mut self, width: usize, height: usize);

    /// Gets the size of the area occupied by the displayable.
    fn get_size(&self) -> (usize, usize);
}
