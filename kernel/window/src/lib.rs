#![no_std]

extern crate event_types;
extern crate dfqueue;

use event_types::Event;
use dfqueue::{DFQueueProducer};

pub trait Window: Send {
    //clean the window on the screen including the border and padding
    fn clean(&self) -> Result<(), &'static str>;

    // check if the pixel is within the window exluding the border and padding
    fn check_in_content(&self, x: usize, y: usize) -> bool;

    // active or inactive a window
    fn active(&mut self, active: bool) -> Result<(), &'static str>;

    // draw the border of the window
    fn draw_border(&self, color: u32) -> Result<(), &'static str>;

    // adjust the size of a window
    fn resize(
        &mut self,
        x: usize,
        y: usize,
        width: usize,
        height: usize,
    ) -> Result<(usize, usize), &'static str>;

    // get the size of content without padding
    fn get_content_size(&self) -> (usize, usize);

    // get the position of content without padding
    fn get_content_position(&self) -> (usize, usize);

    fn key_producer(&mut self) -> &mut DFQueueProducer<Event>;
}