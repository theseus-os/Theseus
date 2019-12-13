#![no_std]

extern crate keycodes_ascii;
extern crate mouse_data;
extern crate alloc;
extern crate shapes;

use keycodes_ascii::KeyEvent;
use mouse_data::MouseEvent;
use alloc::string::String;
use shapes::Coord;

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
    /// An event from another entity that wishes to print a message
    OutputEvent(PrintOutputEvent),
    /// Tells an application that the window manager has resized that application's window
    /// so that it knows to perform any necessary tasks related to window size, such as text reflow.
    WindowResizeEvent(WindowResizeEvent),
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
        Event::OutputEvent(PrintOutputEvent::new(s.into()))
    }

    /// Create a new window resize event
    pub fn new_resize_event(coordinate: Coord, width: usize, height: usize) -> Event {
        Event::WindowResizeEvent(WindowResizeEvent::new(coordinate, width, height))
    }
}

/// use this to deliver input events (such as keyboard input) to the input_event_manager.
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

/// use this to queue up a formatted string that should be printed to the input_event_manager. 
#[derive(Debug, Clone)]
pub struct PrintOutputEvent {
    /// The text to print
    pub text: String,
}

impl PrintOutputEvent {
    /// Create a new print output event. `s` is the string to print
    pub fn new(s: String) -> PrintOutputEvent {
        PrintOutputEvent {
            text: s 
        }
    }
}

/// Use this to inform the window manager to adjust the sizes of existing windows
#[derive(Debug, Clone)]
pub struct WindowResizeEvent {
    /// the new position of the window
    pub coordinate: Coord,
    /// the new width of the window
    pub width: usize, 
    /// the new height of the window
    pub height: usize, 
}

impl WindowResizeEvent {
    /// Create a new window resize event. `coordinate` is the new position and `(width, height)` is the new size of the window.
    pub fn new(coordinate: Coord, width: usize, height: usize) -> WindowResizeEvent {
        WindowResizeEvent {
            coordinate: coordinate,
            width: width, 
            height: height,
        }
    }
}
