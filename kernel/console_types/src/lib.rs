#![no_std]
#![feature(alloc)]

extern crate keycodes_ascii;
extern crate alloc;

use keycodes_ascii::{KeyEvent};
use alloc::string::String;


#[derive(Debug, Clone)]
pub enum ConsoleEvent {
    InputEvent(ConsoleInputEvent),
    OutputEvent(ConsoleOutputEvent),
    ExitEvent,
}

impl ConsoleEvent {
    pub fn new_input_event(kev: KeyEvent) -> ConsoleEvent {
        ConsoleEvent::InputEvent(ConsoleInputEvent::new(kev))
    }

    pub fn new_output_event<S>(s: S, display: bool) -> ConsoleEvent where S: Into<String> {
        ConsoleEvent::OutputEvent(ConsoleOutputEvent::new(s.into(), display))
    }
}

/// use this to deliver input events (such as keyboard input) to the console.
#[derive(Debug, Clone)]
pub struct ConsoleInputEvent {
    pub key_event: KeyEvent,
}

impl ConsoleInputEvent {
    pub fn new(kev: KeyEvent) -> ConsoleInputEvent {
        ConsoleInputEvent {
            key_event: kev,
        }
    }
}



/// use this to queue up a formatted string that should be printed to the console. 
#[derive(Debug, Clone)]
pub struct ConsoleOutputEvent {
    pub text: String,
    // indicates whether or not the terminal/application should refresh its TextDisplay when it handles this output event
    pub display: bool,
}

impl ConsoleOutputEvent {
    pub fn new(s: String, display: bool) -> ConsoleOutputEvent {
        ConsoleOutputEvent {
            text: s,
            display: display
        }
    }
}