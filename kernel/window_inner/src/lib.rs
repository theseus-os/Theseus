//! This crate defines a `WindowInner` struct which implements the `WindowInner` trait.
//!
//! A `WindowInner` object profiles the basic information of a window such as its size, position and other states. It owns a framebuffer which it can display in and render to the final framebuffer via a compositor.

#![no_std]

extern crate alloc;
extern crate mpmc;
extern crate event_types;
extern crate frame_buffer;
extern crate spin;
extern crate shapes;

use alloc::sync::Arc;
use mpmc::Queue;
use event_types::{Event};
use frame_buffer::{FrameBuffer, Pixel, PixelColor};
use shapes::Coord;
use spin::{Mutex};


// The default color of a window;
const WINDOW_DEFAULT_COLOR: PixelColor = 0x80FFFFFF;

/// WindowInner object that should be owned by the manager. It is usually owned by both an application's window and the manager so that the application can modify it and the manager can re-display it when necessary.
pub struct WindowInner<T: Pixel> {
    /// The position of the top-left corner of the window.
    /// It is relative to the top-left corner of the screen.
    pub coordinate: Coord,
    /// The width of the window.
    pub width: usize,
    /// The height of the window.
    pub height: usize,
    /// event consumer that could be used to get event input given to this window
    pub consumer: Queue<Event>, // event input
    pub producer: Queue<Event>, // event output used by window manager
    /// frame buffer of this window
    pub framebuffer: FrameBuffer<T>,
    /// if true, window manager will send all mouse event to this window, otherwise only when mouse is on this window does it send.
    /// This is extremely helpful when application wants to know mouse movement outside itself, because by default window manager only sends mouse event
    /// whether in moving state, only available when it is active. This is set when user press on the title bar (except for the buttons),
    /// and keeping mouse pressed when moving the mouse.
    pub is_moving: bool,
    /// the base position of window moving action, should be the mouse position when `is_moving` is set to true
    pub moving_base: Coord,
}

impl<T: Pixel> WindowInner<T> {

    /// Clear the content of a window
    pub fn clear(&mut self) -> Result<(), &'static str> {
        self.framebuffer.fill_color(T::from(WINDOW_DEFAULT_COLOR));
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

    pub fn get_pixel(&self, coordinate: Coord) -> Result<T, &'static str> {
        self.framebuffer.get_pixel(coordinate)
    }
}

/// Creates a new window object with given position and size
pub fn new_window<'a, T: Pixel>(
    coordinate: Coord,
    framebuffer: FrameBuffer<T>,
) -> Result<Arc<Mutex<WindowInner<T>>>, &'static str> {
    // Init the key input producer and consumer
    let consumer = Queue::with_capacity(100);
    let producer = consumer.clone();

    let (width, height) = framebuffer.get_size();

    // new window object
    let window: WindowInner<T> = WindowInner {
        coordinate: coordinate,
        width: width,
        height: height,
        consumer: consumer,
        producer: producer,
        framebuffer: framebuffer,
        //give_all_mouse_event: false,
        is_moving: false,
        moving_base: Coord::new(0, 0), // the point as a base to start moving
    };

    let window_ref = Arc::new(Mutex::new(window));
    Ok(window_ref)
}