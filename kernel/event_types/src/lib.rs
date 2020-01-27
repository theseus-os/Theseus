#![no_std]

extern crate keycodes_ascii;
extern crate mouse_data;
extern crate alloc;
extern crate shapes;

use keycodes_ascii::KeyEvent;
use mouse_data::MouseEvent;
use alloc::string::String;
use shapes::{Coord, Rectangle};

/// An event describing mouse position rather than movement differential from last event.
/// It contains two position, `coodinate` for the relative position in each window, and `gcoordinate` for global absolute position of the screen.
#[derive(Debug, Clone)]
pub struct MousePositionEvent {
    /// the relative position in window
    pub coordinate: Coord,
    /// the global position in window
    pub gcoordinate: Coord,
    /// whether the mouse is scrolling up
    pub scrolling_up: bool,
    /// whether the mouse is scrolling down
    pub scrolling_down: bool,
    /// whether the left button holds
    pub left_button_hold: bool,
    /// whether the right button holds
    pub right_button_hold: bool,
    /// whether the fourth button holds
    pub fourth_button_hold: bool,
    /// whether the fifth button holds
    pub fifth_button_hold: bool,
}

impl Default for MousePositionEvent  {
    fn default() -> Self {
        MousePositionEvent {
            coordinate: Coord::new(0, 0),
            gcoordinate: Coord::new(0, 0),
            scrolling_up: false,
            scrolling_down: false,
            left_button_hold: false,
            right_button_hold: false,
            fourth_button_hold: false,
            fifth_button_hold: false,
        }
    }
}

#[derive(Debug, Clone)]
pub enum Event {
    /// An input event from a keyboard
    KeyboardEvent(KeyboardInputEvent),
    /// An input event from a mouse
    MouseMovementEvent(MouseEvent),
    /// An event indicating that another entity wants to print the given `String`.
    OutputEvent(String),
    /// Tells an application that the window manager has resized or moved its window
    /// so that it knows to refresh its display and perform any necessary tasks, such as text reflow.
    /// 
    /// The new position and size of the window is given by the `Rectangle` within,
    /// and represents the content area within the window that is accessible to the application,
    /// which excludes the window title bar, borders, etc. 
    WindowResizeEvent(Rectangle),
    /// The event tells application about mouse's position currently (including relative to a window and relative to a screen)
    MousePositionEvent(MousePositionEvent),
    ExitEvent,
}

impl Event {
    /// Create a new keyboard event
    pub fn new_keyboard_event(kev: KeyEvent) -> Event {
        Event::KeyboardEvent(KeyboardInputEvent::new(kev))
    }

    /// Create a new output event
    pub fn new_output_event<S>(s: S) -> Event where S: Into<String> {
        Event::OutputEvent(s.into())
    }

    /// Create a new window resize event
    pub fn new_window_resize_event(new_position: Rectangle) -> Event {
        Event::WindowResizeEvent(new_position)
    }
}

/// A keyboard event, indicating that one or more keys were pressed or released.
#[derive(Debug, Clone)]
pub struct KeyboardInputEvent {
    /// The key input event from i/o device
    pub key_event: KeyEvent,
}

impl KeyboardInputEvent {
    /// Create a new key board input event. `key` is the key input from the keyboard
    pub fn new(key: KeyEvent) -> KeyboardInputEvent {
        KeyboardInputEvent { 
            key_event: key 
        }
    }
}
