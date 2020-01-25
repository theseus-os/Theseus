//! This crate defines a `WindowInner` struct. It profiles the basic information of a window such as its size, position and other states. It owns a framebuffer which it can display in and render to the final framebuffer via a compositor.

#![no_std]

extern crate mpmc;
extern crate event_types;
extern crate framebuffer;
extern crate shapes;
extern crate color;

use mpmc::Queue;
use event_types::{Event};
use framebuffer::{Framebuffer, AlphaPixel};
use color::{Color};
use shapes::{Coord, Rectangle};


// The title bar height, in number of pixels
pub const DEFAULT_TITLE_BAR_HEIGHT: usize = 16;
// left, right, bottom border size, in number of pixels
pub const DEFAULT_BORDER_SIZE: usize = 2;


/// Whether a window is moving (being dragged by the mouse).
pub enum WindowMovingStatus {
    /// The window is not in motion.
    Stationary,
    /// The window is currently in motion. 
    /// The enclosed `Coord` represents the initial position of the window before it started moving.
    Moving(Coord),
}

/// WindowInner object that should be owned by the manager.
/// It is usually owned by both an application's window and the manager so that the application can modify it and the manager can re-display it when necessary.
/// 
/// TODO: fix this dumb structure of too many queues. Currently the `consumer` and `producer` point to the same `Queue`...
pub struct WindowInner {
    /// The position of the top-left corner of the window,
    /// expressed relative to the top-left corner of the screen.
    pub coordinate: Coord,
    /// The width of the border in pixels.
    /// By default, there is a border on the left, right, and bottom edges of the window.
    pub border_size: usize,
    /// The height of title bar in pixels.
    /// By default, there is one title bar at the top edge of the window.
    pub title_bar_height: usize,
    /// event consumer that could be used to get event input given to this window
    pub consumer: Queue<Event>, // event input
    /// event producer that could be used to send events to the `Window` object.
    pub producer: Queue<Event>, // event output used by window manager
    /// The background color of this window, used when initializing or clearing the window's content.
    pub background: Color,
    /// The virtual framebuffer that is used exclusively for rendering only this window.
    pub framebuffer: Framebuffer<AlphaPixel>,
    /// Whether a window is moving or stationary.
    pub moving: WindowMovingStatus,
}

impl WindowInner {
    /// Creates a new `WindowInner` object backed by the given `framebuffer`
    /// and that will be rendered at the given `coordinate` relative to the screen.
    /// 
    /// The given `framebuffer` will be filled with the `background` color.
    pub fn new(
        coordinate: Coord,
        framebuffer: Framebuffer<AlphaPixel>,
        background: Color,
    ) -> Result<WindowInner, &'static str> {
        // Init the key input producer and consumer
        let consumer = Queue::with_capacity(100);
        let producer = consumer.clone();
    
        let mut wi = WindowInner {
            coordinate,
            border_size: DEFAULT_BORDER_SIZE,
            title_bar_height: DEFAULT_TITLE_BAR_HEIGHT,
            consumer,
            producer,
            background,
            framebuffer,
            moving: WindowMovingStatus::Stationary,
        };
        wi.clear()?;
        Ok(wi)
    }

    /// Clear the content of a window by filling it with the `background` color. 
    pub fn clear(&mut self) -> Result<(), &'static str> {
        self.framebuffer.fill(self.background.into());
        Ok(())
    }

    /// Checks if a coordinate relative to the top-left corner of a window is in the window
    pub fn contains(&self, coordinate: Coord) -> bool {
        self.framebuffer.contains(coordinate)
    }

    /// Gets the size of a window in pixels
    pub fn get_size(&self) -> (usize, usize) {
        self.framebuffer.get_size()
    }

    /// Gets the top-left position of the window relative to the top-left of the screen
    pub fn get_position(&self) -> Coord {
        self.coordinate
    }

    /// Sets the top-left position of the window relative to the top-left of the screen
    pub fn set_position(&mut self, coordinate: Coord) {
        self.coordinate = coordinate;
    }

    /// Returns the pixel value at the given `coordinate`,
    /// if the `coordinate` is within the window's bounds.
    pub fn get_pixel(&self, coordinate: Coord) -> Option<AlphaPixel> {
        self.framebuffer.get_pixel(coordinate)
    }

    /// Returns the size of the Window border in pixels. 
    /// There is a border drawn on the left, right, and bottom edges.
    pub fn get_border_size(&self) -> usize {
        self.border_size
    }

    /// Returns the size of the Window title bar in pixels. 
    /// There is a title bar drawn on the top edge of the Window.
    pub fn get_title_bar_height(&self) -> usize {
        self.title_bar_height
    }

    /// Returns the position and dimensions of the Window's content region,
    /// i.e., the area within the window excluding the title bar and border.
    /// 
    /// The returned `Rectangle` is expressed relative to this Window's position.
    pub fn content_area(&self) -> Rectangle {
        let (window_width, window_height) = self.get_size();
        // There is one title bar on top, and a border on the left, right, and bottom
        let top_left = Coord::new(self.border_size as isize, self.title_bar_height as isize);
        let bottom_right = Coord::new((window_width - self.border_size) as isize, (window_height - self.border_size) as isize);
        Rectangle { top_left, bottom_right }
    }

    /// Resizes and moves this window to fit the given `Rectangle` that describes its new position. 
    pub fn resize(&mut self, new_position: Rectangle) -> Result<(), &'static str> {
        // First, perform the actual resize of the inner window
        self.coordinate = new_position.top_left;
        self.framebuffer = Framebuffer::new(new_position.width(), new_position.height(), None)?;

        // Second, send a resize event to that application window so it knows to refresh its display.
        // Instead of sending the total size of the whole window, 
        // we instead send the size and position of the inner content area of the window. 
        // This prevents the application from thinking it can render over the window's title bar or border.
        self.producer.push(Event::new_window_resize_event(self.content_area()))
            .map_err(|_e| "Failed to enqueue the new resize event")?;

        Ok(())
    }
}
