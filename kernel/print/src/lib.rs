#![no_std]
#![feature(alloc)]
extern crate input_event_manager;


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
        $crate::call_input_event_manager(format_args!($($arg)*));
    });
}

use core::fmt;
/// Calls print_to_stdout_args inside the input event manager crate so that the print event can be flagged
/// to refresh the TextDisplay or not
pub fn call_input_event_manager(fmt_args: fmt::Arguments) {
    input_event_manager::print_to_stdout_args(fmt_args)
}