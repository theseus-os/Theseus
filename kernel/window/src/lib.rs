//! This crate defines a `Window` trait.
//! A window manager maintains a list of `Window` objects.

#![no_std]

extern crate event_types;
extern crate dfqueue;
extern crate frame_buffer;

use event_types::Event;
use dfqueue::{DFQueueProducer};
use frame_buffer::RelativeCoord;

/// The `Window` trait.
pub trait Window: Send {
    /// Clears the window on the screen including the border and padding.
    fn clear(&self) -> Result<(), &'static str>;

    /// Checks if the coordinate relative to the window is within the window exluding the border and padding.
    fn contains_coordinate(&self, point: RelativeCoord) -> bool;

    /// Sets the window as active or not.
    fn set_active(&mut self, active: bool) -> Result<(), &'static str>;

    /// Draws the border of the window.
    fn draw_border(&self, color: u32) -> Result<(), &'static str>;

    /// Adjusts the size (width, height) and coordinate of the window relative to the screen.
    fn resize(
        &mut self,
        coordinate: RelativeCoord,
        width: usize,
        height: usize,
    ) -> Result<(usize, usize), &'static str>;

    /// Gets the size of content without padding.
    fn get_content_size(&self) -> (usize, usize);

    /// Gets the coordinate of content relative to the window without padding.
    fn get_content_position(&self) -> RelativeCoord;

    /// Gets the producer of events.
    fn events_producer(&mut self) -> &mut DFQueueProducer<Event>;
}