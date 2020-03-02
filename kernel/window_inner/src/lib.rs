//! The `WindowInner` struct is the internal representation of a `Window` used by the window manager. 
//! 
//! In comparison, the `Window` struct is application-facing, meaning it is used by (owned by)
//! and exposed directly to applications or tasks that wish to display content. 
//! 
//! The `WindowInner` in the window_manager-facing version of the `Window`, 
//! and each `Window` contains a reference to its `WindowInner`. 
//! 
//! The window manager typically holds `Weak` references to a `WindowInner` struct,
//! which allows it to control the window itself and handle non-application-related components of the window,
//! such as the title bar, border, etc. 
//! 
//! It also allows the window manager to control the window, e.g., move, hide, show, or resize it
//! in a way that applications may not be able to do.

#![no_std]

extern crate mpmc;
extern crate event_types;
extern crate framebuffer;
extern crate shapes;

use mpmc::Queue;
use event_types::{Event};
use framebuffer::{Framebuffer, AlphaPixel};
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

/// The `WindowInner` struct is the internal system-facing representation of a window. 
/// Its members and functions describe the size, state, and events related to window handling,
/// including elements like:
/// * The underlying virtual framebuffer to which the window is rendered,
/// * THe location and dimensions of the window in the final screen, 
/// * The window's title bar, buttons, and borders, 
/// * Queues for events that have been received by this window, and more. 
/// 
/// The window manager directly interacts with instances of `WindowInner` rather than `Window`,
/// and the application tasks should not have direct access to this struct for correctness reasons.
/// See the crate-level documentation for more details about how to use this
/// and how it differs from `Window`.
pub struct WindowInner {
    /// The position of the top-left corner of the window,
    /// expressed relative to the top-left corner of the screen.
    coordinate: Coord,
    /// The width of the border in pixels.
    /// By default, there is a border on the left, right, and bottom edges of the window.
    pub border_size: usize,
    /// The height of title bar in pixels.
    /// By default, there is one title bar at the top edge of the window.
    pub title_bar_height: usize,
    /// The producer side of this window's event queue. 
    /// Entities that want to send events to this window (or the application that owns this window) 
    /// should push events onto this queue.
    /// 
    /// The corresponding consumer for this event queue is found in the `Window` struct
    /// that created and owns this `WindowInner` instance.
    event_producer: Queue<Event>, // event output used by window manager
    /// The virtual framebuffer that is used exclusively for rendering only this window.
    framebuffer: Framebuffer<AlphaPixel>,
    /// Whether a window is moving or stationary.
    /// 
    /// TODO: FIXME (kevinaboos): this should be private, and window moving logic should be moved into this crate.
    pub moving: WindowMovingStatus,
}

impl WindowInner {
    /// Creates a new `WindowInner` object backed by the given `framebuffer`
    /// and that will be rendered at the given `coordinate` relative to the screen.
    pub fn new(
        coordinate: Coord,
        framebuffer: Framebuffer<AlphaPixel>,
        event_producer: Queue<Event>,
    ) -> WindowInner {
        WindowInner {
            coordinate,
            border_size: DEFAULT_BORDER_SIZE,
            title_bar_height: DEFAULT_TITLE_BAR_HEIGHT,
            event_producer,
            framebuffer,
            moving: WindowMovingStatus::Stationary,
        }
    }

    /// Returns `true` if the given `coordinate` (relative to the top-left corner of this window)
    /// is within the bounds of this window.
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

    /// Returns an immutable reference to this window's virtual Framebuffer. 
    pub fn framebuffer(&self) -> &Framebuffer<AlphaPixel> {
        &self.framebuffer
    }

    /// Returns a mutable reference to this window's virtual Framebuffer. 
    pub fn framebuffer_mut(&mut self) -> &mut Framebuffer<AlphaPixel> {
        &mut self.framebuffer
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

        // Second, send a resize event to that application window (the `Window` object) 
        // so it knows to refresh its display.
        // Rather than send the total size of the whole window, 
        // we instead send the size and position of the inner content area of the window. 
        // This prevents the application from thinking it can render over the area
        // that contains this window's title bar or border.
        self.send_event(Event::new_window_resize_event(self.content_area()))
            .map_err(|_e| "Failed to enqueue the resize event; window event queue was full.")?;

        Ok(())
    }

    /// Sends the given `event` to this window.
    /// 
    /// If the event queue was full, `Err(event)` is returned.
    pub fn send_event(&self, event: Event) -> Result<(), Event> {
        self.event_producer.push(event)
    }
}
