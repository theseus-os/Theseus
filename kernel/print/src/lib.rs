#![no_std]
#![feature(alloc)]
extern crate terminal;

#[macro_use] extern crate alloc;
#[macro_use] extern crate log;


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
        $crate::print_to_stdout_args(format_args!($($arg)*));
    });
}


use core::fmt;
/// Converts the given `core::fmt::Arguments` to a `String` and queues it up to be printed out to the input_event_manager. FIX THIS
/// This function is currently in the input_event_manager crate because this crate is the only one that is aware of the focused terminal window
pub fn print_to_stdout_args(fmt_args: fmt::Arguments) {
    use core::fmt::{Write, Display, Debug, Formatter};
    // Passes the current terminal number (the one being focused on) to whoever is printing so it knows whether or not to refresh its display
    let _result = terminal::print_to_stdout(format!("{}", fmt_args));
}