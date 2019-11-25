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
use frame_buffer::{FrameBuffer, Coord, RectArea};

/// Trait for displayables. A displayable is an item which can display itself onto a framebuffer. 
/// It is usually a composition of basic graphs and can act as a component such as a text box, a button belonging to a window, etc. 
pub trait Displayable: Downcast + Send {
    /// Displays in a framebuffer.
    /// # Arguments
    /// * `coordinate`: the coordinate within the given `framebuffer` where this dis    playable should render itself. The `coordinate` is relative to the top-left point `(0, 0)` of the `framebuffer`.
    /// * `framebuffer`: the framebuffer to display onto. Display in default framebuffer of the displayable if this argument is `None`.
    ///
    /// Returns a list of updated blocks. The tuple (index, width) represents the index of the block in the framebuffer and its width. The use of `block` is described in the `frame_buffer_compositor` crate.
    fn display(
        &mut self,
        coordinate: Coord,
        framebuffer: Option<&mut dyn FrameBuffer>,
    ) -> Result<RectArea, &'static str> ;

    fn clear(
        &mut self,
        coordinate: Coord,
        framebuffer: Option<&mut dyn FrameBuffer>,
    ) -> Result<(), &'static str> ;

    /// Resizes the displayable area.
    fn resize(&mut self, width: usize, height: usize);

    /// Gets the size of the area occupied by the displayable.
    fn get_size(&self) -> (usize, usize);

    /// Get the position of the displayable in its container.
    fn get_position(&self) -> Coord {
        Coord::new(0, 0)
    }

    /// Transmute the displayable to a mutable text displayable. Return error if the object does not implement `TextDisplayable`.
    fn as_text_mut(&mut self) -> Result<&mut dyn TextDisplayable, &'static str> {
        Err("The displayable is not a text displayable")
    }

    /// Transmute the displayable to a text displayable. Return error if the object does not implement `TextDisplayable`.
    fn as_text(&self) -> Result<&dyn TextDisplayable, &'static str> {
        Err("The displayable is not a text displayable")
    }

}
impl_downcast!(Displayable);

/// Trait for text displayables. A text displayable is a box of text. It can display its inner content onto a framebuffer like other displayables.
pub trait TextDisplayable: Displayable {
    /// Get the size of the text displayable in units of characters.
    fn get_dimensions(&self) -> (usize, usize);
    
    /// Gets the position of the next symbol as index in units of characters.
    fn get_next_index(&self) -> usize;

    /// Set `text` as the inner content of the text displayable
    fn set_text(&mut self, text: &str);

}
impl_downcast!(TextDisplayable);
