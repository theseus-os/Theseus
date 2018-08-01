//! Allows crates in the kernel to log messages to the default terminal using the 
//! println! and print! macros
//! 
//! The current system simply enqueus the Print event into the print queue of the default terminal

#![no_std]
#![feature(alloc)]
extern crate input_event_manager;
extern crate spin;
extern crate dfqueue;
extern crate event_types;
#[macro_use] extern crate alloc;

use dfqueue::DFQueueProducer;
use spin::Once;
use event_types::Event;

pub static DEFAULT_TERMINAL_QUEUE: Once<DFQueueProducer<Event>> = Once::new();
// / Calls `print!()` with an extra newilne ('\n') appended to the end. 
#[macro_export]
macro_rules! println {
    ($fmt:expr) => (print!(concat!($fmt, "\n")));
    ($fmt:expr, $($arg:tt)*) => (print!(concat!($fmt, "\n"), $($arg)*));
}
/// The main printing macro, which simply pushes an output event to the input_event_manager's event queue. 
/// This ensures that only one thread (the input_event_manager acting as a consumer) ever accesses the GUI.
#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => ({
        $crate::print_default_term(format_args!($($arg)*));
    });
}
use core::fmt;
/// Calls print_to_stdout_args inside the input event manager crate so that the print event can be flagged
/// to refresh the TextDisplay or not
pub fn print_default_term(fmt_args: fmt::Arguments) {
    if let Some(q) = DEFAULT_TERMINAL_QUEUE.try() {
        let _ = q.enqueue(Event::new_output_event(format!("{}", fmt_args)));
    }
}
