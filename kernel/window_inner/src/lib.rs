//! This crate defines a `WindowInner` struct. It profiles the basic information of a window such as its size, position and other states. It owns a framebuffer which it can display in and render to the final framebuffer via a compositor.

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
use frame_buffer::{FrameBuffer, AlphaPixel, AlphaColor};
use shapes::Coord;
use spin::{Mutex};


/// The default color of a window;
const WINDOW_DEFAULT_COLOR: u32 = 0x80FFFFFF;

/// The status about whether a window is moving
pub enum WindowMovingStatus {
    /// marks a non-moving window.
    Stationary,
    /// marks a moving window. The inner coordinate is the position before the window starts to move.
    Moving(Coord),
}

/// WindowInner object that should be owned by the manager. It is usually owned by both an application's window and the manager so that the application can modify it and the manager can re-display it when necessary.
pub struct WindowInner {
    /// The position of the top-left corner of the window.
    /// It is relative to the top-left corner of the screen.
    pub coordinate: Coord,
    /// The width of the window.
    pub width: usize,
    /// The height of the window.
    pub height: usize,
    /// event consumer that could be used to get event input given to this window
    pub consumer: Queue<Event>, // event input
    /// event producer that could be used to send events to the `Window` object.
    pub producer: Queue<Event>, // event output used by window manager
    /// frame buffer of this window
    pub framebuffer: FrameBuffer<AlphaPixel>,
    /// Whether a window is moving and the position before a window starts to move.
    pub moving: WindowMovingStatus,
}

impl WindowInner {

    /// Clear the content of a window
    pub fn clear(&mut self) -> Result<(), &'static str> {
        self.framebuffer.fill_color(AlphaColor::from(WINDOW_DEFAULT_COLOR).into());
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

    /// Returns the pixel at the coordinate
    pub fn get_pixel(&self, coordinate: Coord) -> Result<AlphaPixel, &'static str> {
        self.framebuffer.get_pixel(coordinate)
    }
}

/// Creates a new window object with given position and size
pub fn new_window<'a>(
    coordinate: Coord,
    framebuffer: FrameBuffer<AlphaPixel>,
) -> Result<Arc<Mutex<WindowInner>>, &'static str> {
    // Init the key input producer and consumer
    let consumer = Queue::with_capacity(100);
    let producer = consumer.clone();

    let (width, height) = framebuffer.get_size();

    // new window object
    let window: WindowInner = WindowInner {
        coordinate: coordinate,
        width: width,
        height: height,
        consumer: consumer,
        producer: producer,
        framebuffer: framebuffer,
        moving: WindowMovingStatus::Stationary,
    };

    let window_ref = Arc::new(Mutex::new(window));
    Ok(window_ref)
}