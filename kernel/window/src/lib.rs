//! This crate defines a `Window` trait.
//! A window manager maintains a list of `Window` objects.

#![no_std]

extern crate alloc;
extern crate dfqueue;
extern crate displayable;
extern crate event_types;
extern crate frame_buffer;
extern crate frame_buffer_compositor;
#[macro_use] extern crate downcast_rs;

use alloc::boxed::Box;
use alloc::vec::IntoIter;
use dfqueue::{DFQueueConsumer, DFQueueProducer};
use displayable::Displayable;
use downcast_rs::Downcast;
use event_types::Event;
use frame_buffer::{Coord, FrameBuffer, Pixel, RectArea};
use frame_buffer_compositor::{Block};

/// Trait for window profile. A window manager holds a list of objects who implement the `Window` trait.
/// A `Window` provides states required by the window manager such as the size, the loaction and the active state of a window.
pub trait Window {
    /// Clears the window on the screen including the border and padding.
    fn clear(&mut self) -> Result<(), &'static str>;

    /// Checks if the coordinate relative to the top-left corner of the window is within it exluding the border and padding.
    fn contains(&self, coordinate: Coord) -> bool;

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
    fn get_position(&self) -> Coord;

    /// Gets the producer of events.
    fn events_producer(&mut self) -> &mut DFQueueProducer<Event>;

    /// Sets the top-left position of the window relative to the top-left point of the screen
    fn set_position(&mut self, coordinate: Coord);

    /// Gets the top-left position of the window relative to the top-left point of the screen before moving. Only used in display subsystem which is able to handle mous events.
    fn get_moving_base(&self) -> Coord;

    /// Sets the top-left position of the window relative to the top-left point of the screen before moving. Only used in display subsystem which is able to handle mous events.
    fn set_moving_base(&mut self, coordinate: Coord);

    /// Wether the window is moving by a mouse
    fn is_moving(&self) -> bool;

    /// Sets wether the window is moving by a mouse
    fn set_is_moving(&mut self, moving: bool);

    /// Sets wether the mouse is in the window. Useful in deciding whether the window or the whole desktop should react the a window event.
    fn set_give_all_mouse_event(&mut self, flag: bool);


    /// Wether the mouse is in the window. Useful in deciding whether the window or the whole desktop should react the a window event.
    fn give_all_mouse_event(&mut self) -> bool;

    /// Gets a pixel at `coordinate` relative to the top-left corner of the window. 
    fn get_pixel(&self, _coordinate: Coord) -> Result<Pixel, &'static str> {
        Err("get_pixel() is not implement for this window")
    }

    fn framebuffer(&self) -> &dyn FrameBuffer;

    fn framebuffer_mut(&mut self) -> &mut dyn FrameBuffer;

    fn coordinate(&self) -> Coord;
}