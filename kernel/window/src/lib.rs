//! This crate defines a `Window` trait.
//! A window manager maintains a list of `Window` objects.

#![no_std]

extern crate alloc;
extern crate dfqueue;
extern crate displayable;
extern crate event_types;
extern crate frame_buffer;
#[macro_use]
extern crate downcast_rs;

use alloc::boxed::Box;
use alloc::vec::IntoIter;
use dfqueue::{DFQueueConsumer, DFQueueProducer};
use displayable::Displayable;
use downcast_rs::Downcast;
use event_types::Event;
use frame_buffer::{Coord, FrameBuffer, Pixel};

/// Trait for windows. A window manager holds a list of objects who implement the `Window` trait.
/// A `Window` provides states required by the window manager such as the size, the loaction and the active state of a window.
pub trait WindowProfile {
    /// Clears the window on the screen including the border and padding.
    fn clear(&mut self) -> Result<(), &'static str>;

    // Checks if the coordinate relative to the top-left corner of the window is within it exluding the border and padding.
    fn contains(&self, coordinate: Coord) -> bool;

    // Sets the window as active or not.
    //fn set_active(&mut self, active: bool) -> Result<(), &'static str>;

    /// Draws the border of the window.
    fn draw_border(&self, color: u32) -> Result<(), &'static str>;

    /// Adjusts the size (width, height) and coordinate of the window relative to the top-left corner of the screen.
    fn resize(
        &mut self,
        coordinate: Coord,
        width: usize,
        height: usize,
    ) -> Result<(usize, usize), &'static str>;

    /// Gets the size of content without padding.
    fn get_content_size(&self) -> (usize, usize);

    /// Gets the coordinate of content relative to top-left corner of the window without padding.
    fn get_content_position(&self) -> Coord;

    /// Gets the producer of events.
    fn events_producer(&mut self) -> &mut DFQueueProducer<Event>;

    fn set_position(&mut self, coordinate: Coord);

    fn get_moving_base(&self) -> Coord;

    fn set_moving_base(&mut self, coordinate: Coord);

    fn is_moving(&self) -> bool;

    fn set_is_moving(&mut self, moving: bool);

    fn set_give_all_mouse_event(&mut self, flag: bool);

    fn give_all_mouse_event(&mut self) -> bool;

    fn get_pixel(&self, coordinate: Coord) -> Result<Pixel, &'static str> {
        Err("get_pixel() is not implement for this window")
    }
}

pub trait Window: Downcast + Send {
    fn consumer(&mut self) -> &mut DFQueueConsumer<Event>;

    /// React to window relative events such as move the window
    fn handle_event(&mut self) -> Result<(), &'static str>;

    fn get_background(&self) -> Pixel;

    fn get_displayable_mut(
        &mut self,
        display_name: &str,
    ) -> Result<&mut Box<dyn Displayable>, &'static str>;

    fn get_displayable(&self, display_name: &str) -> Result<&Box<dyn Displayable>, &'static str>;

    fn framebuffer(&mut self) -> Option<&mut dyn FrameBuffer>;

    fn display(&mut self, display_name: &str) -> Result<(), &'static str>;

    fn get_displayable_position(&self, key: &str) -> Result<Coord, &'static str>;

    fn render(&mut self, blocks: Option<IntoIter<(usize, usize)>>) -> Result<(), &'static str>;

    fn add_displayable(
        &mut self,
        key: &str,
        coordinate: Coord,
        displayable: Box<dyn Displayable>,
    ) -> Result<(), &'static str>;
}
impl_downcast!(Window);
