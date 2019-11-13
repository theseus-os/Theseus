//! This crate defines a trait of `Displayable`.
//! A displayable is a block of content. It can display itself onto a framebuffer.
//! The coordinate of a displayable represents its origin (top-left point).

#![no_std]

extern crate alloc;
extern crate frame_buffer;
#[macro_use]
extern crate downcast_rs;

use alloc::boxed::Box;
use alloc::vec::Vec;
use downcast_rs::Downcast;
use frame_buffer::{FrameBuffer, Coord};

/// Trait for displayables. A displayable is an item which can display itself onto a framebuffer. 
/// It is usually a composition of basic graphs and can act as a component such as a text box, a button belonging to a window, etc. 
pub trait Displayable: Downcast + Send {
    /// Displays in a framebuffer.
    /// # Arguments
    /// * `coordinate`: the coordinate within the given `framebuffer` where this dis    playable should render itself. The `coordinate` is relative to the top-left point `(0, 0)` of the `framebuffer`.
    /// * `framebuffer`: the framebuffer to display onto.
    ///
    /// Returns a list of updated blocks. The tuple (index, width) represents the index of the block in the framebuffer and its width. The use of `block` is described in the `frame_buffer_compositor` crate.
    fn display_in(
        &mut self,
        coordinate: Coord,
        framebuffer: &mut dyn FrameBuffer,
    ) -> Result<Vec<(usize, usize)>, &'static str> ;

    fn display(&mut self) -> Result<(), &'static str>;
    /// Resizes the displayable area.
    fn resize(&mut self, width: usize, height: usize);

    /// Gets the size of the area occupied by the displayable.
    fn get_size(&self) -> (usize, usize);

    fn get_position(&self) -> Coord {
        Coord::new(0, 0)
    }

    fn as_text_mut(&mut self) -> Result<&mut TextDisplayable, &'static str> {
        Err("The displayable is not a text displayable")
    }
}
impl_downcast!(Displayable);


pub trait TextDisplayable: Displayable {
    fn get_dimensions(&self) -> (usize, usize);
        /// Gets the position of the next symbol as index in units of characters.
    fn get_next_index(&self) -> usize;

    fn set_text(&mut self, slice: &str);
}
