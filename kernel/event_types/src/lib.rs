#![no_std]

extern crate alloc;
extern crate frame_buffer;
extern crate keycodes_ascii;
extern crate mouse_data;

use alloc::string::String;
use frame_buffer::Coord;
use keycodes_ascii::KeyEvent;
use mouse_data::MouseEvent;

/// A event describe mouse position rather than movement differential from last event.
/// It contains two position, (x,y) for the relative position in each window, and (gx,gy) for global absolute position of the screen.
#[derive(Debug, Clone)]
pub struct MousePositionEvent {
    // tells window application of the cursor information
    /// the relative position in window
    pub coordinate: Coord,
    /// the global position in window
    pub gcoordinate: Coord,
    /// the global position in window
    pub scrolling_up: bool,
    pub scrolling_down: bool,
    pub left_button_hold: bool,
    pub right_button_hold: bool,
    pub fourth_button_hold: bool,
    pub fifth_button_hold: bool,
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
    /// The event tells application about cursor's position currently (including relative to a window and relative to a screen)
    MousePositionEvent(MousePositionEvent),
    ExitEvent,
}

impl Event {
    pub fn new_keyboard_event(kev: KeyEvent) -> Event {
        Event::KeyboardEvent(KeyboardInputEvent::new(kev))
    }

    pub fn new_output_event<S>(s: S) -> Event
    where
        S: Into<String>,
    {
        Event::OutputEvent(PrintOutputEvent::new(s.into()))
    }

    pub fn new_resize_event(coordinate: Coord, width: usize, height: usize) -> Event {
        Event::WindowResizeEvent(WindowResizeEvent::new(coordinate, width, height))
    }
}

/// use this to deliver input events (such as keyboard input) to the input_event_manager.
#[derive(Debug, Clone)]
pub struct KeyboardInputEvent {
    pub key_event: KeyEvent,
}

impl KeyboardInputEvent {
    pub fn new(kev: KeyEvent) -> KeyboardInputEvent {
        KeyboardInputEvent { key_event: kev }
    }
}

/// use this to queue up a formatted string that should be printed to the input_event_manager.
#[derive(Debug, Clone)]
pub struct PrintOutputEvent {
    pub text: String,
}

impl PrintOutputEvent {
    pub fn new(s: String) -> PrintOutputEvent {
        PrintOutputEvent { text: s }
    }
}

//Use this to inform the window manager to adjust the sizes of existing windows
#[derive(Debug, Clone)]
pub struct WindowResizeEvent {
    pub coordinate: Coord,
    pub width: usize,
    pub height: usize,
}
impl WindowResizeEvent {
    pub fn new(coordinate: Coord, width: usize, height: usize) -> WindowResizeEvent {
        WindowResizeEvent {
            coordinate: coordinate,
            width: width,
            height: height,
        }
    }
}
