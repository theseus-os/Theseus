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
    /// Cleans the window on the screen including the border and padding.
    fn clean(&self) -> Result<(), &'static str>;

    /// Checks if the pixel (x, y) is within the window exluding the border and padding.
    fn check_in_content(&self, point: RelativeCoord) -> bool;

    /// Actives or inactives a window.
    fn active(&mut self, active: bool) -> Result<(), &'static str>;

    /// Draws the border of the window.
    fn draw_border(&self, color: u32) -> Result<(), &'static str>;

    /// Adjusts the size (width, height) and position (x, y) of a window.
    fn resize(
        &mut self,
        x: usize,
        y: usize,
        width: usize,
        height: usize,
    ) -> Result<(usize, usize), &'static str>;

    /// Gets the size of content without padding.
    fn get_content_size(&self) -> (usize, usize);

    /// Gets the position of content without padding.
    fn get_content_position(&self) -> (usize, usize);

    /// Gets the producer of key inputs.
    fn key_producer(&mut self) -> &mut DFQueueProducer<Event>;
}