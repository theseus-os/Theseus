#![no_std]
#![feature(alloc)]

extern crate keycodes_ascii;
extern crate alloc;

use keycodes_ascii::{KeyEvent};
use alloc::string::String;


#[derive(Debug, Clone)]
pub enum Event {
    InputEvent(KeyboardInputEvent),
    OutputEvent(PrintOutputEvent),
    ResizeEvent(WindowResizeEvent), // tuple containing x, y, width, and height arguments for resizing the window
    DisplayEvent,
    ExitEvent,
}

impl Event {
    pub fn new_input_event(kev: KeyEvent) -> Event {
        Event::InputEvent(KeyboardInputEvent::new(kev))
    }

    pub fn new_output_event<S>(s: S) -> Event where S: Into<String> {
        Event::OutputEvent(PrintOutputEvent::new(s.into()))
    }

    pub fn new_resize_event(x: usize, y: usize, width: usize, height: usize) -> Event {
        Event::ResizeEvent(WindowResizeEvent::new(x,y,width, height))
    }
}

/// use this to deliver input events (such as keyboard input) to the input_event_manager.
#[derive(Debug, Clone)]
pub struct KeyboardInputEvent {
    pub key_event: KeyEvent,
}

impl KeyboardInputEvent {
    pub fn new(kev: KeyEvent) -> KeyboardInputEvent {
        KeyboardInputEvent {
            key_event: kev,
        }
    }
}

/// use this to queue up a formatted string that should be printed to the input_event_manager. 
#[derive(Debug, Clone)]
pub struct PrintOutputEvent {
    pub text: String,
}

impl PrintOutputEvent {
    pub fn new(s: String) -> PrintOutputEvent {
        PrintOutputEvent {
            text: s,
        }
    }
}

#[derive(Debug, Clone)]
pub struct WindowResizeEvent {
    pub x: usize,
    pub y: usize,
    pub width: usize, 
    pub height: usize, 
}

impl WindowResizeEvent {
    pub fn new(x: usize, y: usize, width: usize, height:usize) -> WindowResizeEvent {
        WindowResizeEvent {
            x: x,
            y: y,
            width: width, 
            height: height,
        }
    }
}