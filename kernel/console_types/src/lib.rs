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
    DisplayEvent,
    ExitEvent,
}

impl ConsoleEvent {
    pub fn new_input_event(kev: KeyEvent) -> ConsoleEvent {
        ConsoleEvent::InputEvent(ConsoleInputEvent::new(kev))
    }

    pub fn new_output_event<S>(s: S) -> ConsoleEvent where S: Into<String> {
        // x: Option<usize> is the terminal reference number. Used so that this output event is outputted to the correct Terminal's VGA buffer
        // Matches with Terminal.term_ref
        ConsoleEvent::OutputEvent(ConsoleOutputEvent::new(s.into()))
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
}

impl ConsoleOutputEvent {
    pub fn new(s: String) -> ConsoleOutputEvent {
        ConsoleOutputEvent {
            text: s,
        }
    }
}