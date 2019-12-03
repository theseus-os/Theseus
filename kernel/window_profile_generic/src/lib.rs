//! This crate defines a `WindowProfileGeneric` struct which implements the `WindowProfileGeneric` trait.
//!
//! A `WindowProfileGeneric` object profiles the basic information of a window such as its size, position and other states. It owns a framebuffer which it can display in and render to the final framebuffer via a compositor.

#![no_std]

extern crate alloc;
extern crate dfqueue;
extern crate event_types;
extern crate frame_buffer;
extern crate spin;

use alloc::boxed::Box;
use alloc::sync::Arc;
use core::ops::{Deref, DerefMut};
use dfqueue::{DFQueue, DFQueueConsumer, DFQueueProducer};
use event_types::{Event};
use frame_buffer::{Coord, FrameBuffer, Pixel};
use spin::{Mutex};

// The default color of a window;
const WINDOW_DEFAULT_COLOR: Pixel = 0x80FFFFFF;

/// Window object that should be owned by the manager. It implements the `Window` trait.
pub struct WindowProfileGeneric {
    /// The position of the top-left corner of the window.
    /// It is relative to the top-left corner of the screen.
    pub coordinate: Coord,
    /// The width of the window.
    pub width: usize,
    /// The height of the window.
    pub height: usize,
    /// event consumer that could be used to get event input given to this window
    pub consumer: DFQueueConsumer<Event>, // event input
    pub producer: DFQueueProducer<Event>, // event output used by window manager
    /// frame buffer of this window
    pub framebuffer: Box<dyn FrameBuffer + Send>,
    /// if true, window manager will send all mouse event to this window, otherwise only when mouse is on this window does it send.
    /// This is extremely helpful when application wants to know mouse movement outside itself, because by default window manager only sends mouse event
    /// when mouse is in the window's region. This is used when user move the window, to receive mouse event when mouse is out of the current window.
    pub give_all_mouse_event: bool,
    /// whether in moving state, only available when it is active. This is set when user press on the title bar (except for the buttons),
    /// and keeping mouse pressed when moving the mouse.
    pub is_moving: bool,
    /// the base position of window moving action, should be the mouse position when `is_moving` is set to true
    pub moving_base: Coord,
}

impl WindowProfileGeneric {

    /// Clear the content of a window
    pub fn clear(&mut self) -> Result<(), &'static str> {
        self.framebuffer.fill_color(WINDOW_DEFAULT_COLOR);
        Ok(())
    }

    /// Draw the border of a window
    pub fn draw_border(&self, _color: u32) -> Result<(), &'static str> {
        // this window uses Window instead of border
        Ok(())
    }

    /// Checks if a coordinate relative to the top-left corner of a window is in the window
    pub fn contains(&self, coordinate: Coord) -> bool {
        self.framebuffer.contains(coordinate)
    }

    /// Gets the size of a window in pixels
    pub fn get_size(&self) -> (usize, usize) {
        (self.width, self.height)
    }

    /// Gets the top-left position of the window relative to the top-left of the screen
    pub fn get_position(&self) -> Coord {
        self.coordinate
    }

    /// Sets the top-left position of the window relative to the top-left of the screen
    pub fn set_position(&mut self, coordinate: Coord) {
        self.coordinate = coordinate;
    }

    pub fn get_pixel(&self, coordinate: Coord) -> Result<Pixel, &'static str> {
        self.framebuffer.get_pixel(coordinate)
    }
}

/// Creates a new window object with given position and size
pub fn new_window<'a>(
    coordinate: Coord,
    framebuffer: Box<dyn FrameBuffer + Send>,
) -> Result<Arc<Mutex<WindowProfileGeneric>>, &'static str> {
    // Init the key input producer and consumer
    let consumer = DFQueue::new().into_consumer();
    let producer = consumer.obtain_producer();

    let (width, height) = framebuffer.get_size();

    // new window object
    let window: WindowProfileGeneric = WindowProfileGeneric {
        coordinate: coordinate,
        width: width,
        height: height,
        consumer: consumer,
        producer: producer,
        framebuffer: framebuffer,
        give_all_mouse_event: false,
        is_moving: false,
        moving_base: Coord::new(0, 0), // the point as a base to start moving
    };

    let window_ref = Arc::new(Mutex::new(window));
    Ok(window_ref)
}