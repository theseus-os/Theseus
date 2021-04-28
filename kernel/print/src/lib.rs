//! Allows crates in the kernel to log messages to the default terminal using the 
//! println! and print! macros
//! 
//! The current system simply enqueus the Print event into the print queue of the default terminal

#![no_std]

#[macro_use] extern crate alloc;
extern crate spin;
extern crate dfqueue;
extern crate event_types;

use core::fmt;
use spin::Once;
use dfqueue::DFQueueProducer;
use event_types::Event;


/// The kernel's default destination for print/println invocations
static DEFAULT_PRINT_OUTPUT: Once<DFQueueProducer<Event>> = Once::new();

/// Gives the kernel an endpoint (queue producer) to which it can send messages to be printed
pub fn set_default_print_output(producer: DFQueueProducer<Event>) {
    DEFAULT_PRINT_OUTPUT.call_once(|| producer);
}

/// Calls `print!()` with an extra newilne `\n` appended to the end. 
#[macro_export]
macro_rules! println {
    ($fmt:expr) => (print!(concat!($fmt, "\n")));
    ($fmt:expr, $($arg:tt)*) => (print!(concat!($fmt, "\n"), $($arg)*));
}

/// The main printing macro, which simply pushes an output event to the event queue. 
/// This ensures that only one thread (e.g., a terminal acting as a consumer) ever accesses the GUI.
#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => ({
        $crate::print_to_default_output(format_args!($($arg)*));
    });
}

/// Enqueues the given `fmt_args` as a String onto the default printing output queue,
/// which is typically the default terminal application's input queue
pub fn print_to_default_output(fmt_args: fmt::Arguments) {
    if let Some(q) = DEFAULT_PRINT_OUTPUT.get() {
        let _ = q.enqueue(Event::new_output_event(format!("{}", fmt_args)));
    }
}
