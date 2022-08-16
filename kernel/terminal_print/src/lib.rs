//! Applications that want to print to their parent terminal must import this crate
//! To print, applications can call the println! or print! macros
//! 
//! Printing simulates the parent-child relationship for standard out when parent applications spawn child applications
//! For example, when a terminal runs a command, it will map that child command's task ID to that terminal's print producer
//! so that any output from the child command is identified and outputted to the parent terminal
//! 
//! # Deprecated
//! This crate is deprecated in favor of using proper stdio streams. 
//! 
//! # Note
//! This printing crate only supports single-task applications. 
//! If an application itself spawns child tasks, those will not be able to properly print through these interfaces.

#![no_std]
#![feature(const_btree_new)]

#[macro_use] extern crate alloc;
extern crate logger;
extern crate task;
extern crate dfqueue;
extern crate event_types;
extern crate spin;

use event_types::Event;
use dfqueue::DFQueueProducer;
use alloc::collections::BTreeMap;
use spin::Mutex;

/// Calls `print!()` with an extra newline ('\n') appended to the end. 
#[macro_export]
macro_rules! println {
    ($fmt:expr) => (print!(concat!($fmt, "\n")));
    ($fmt:expr, $($arg:tt)*) => (print!(concat!($fmt, "\n"), $($arg)*));

}

/// The main printing macro, which simply pushes an output event to the current task's terminal event queue. 
/// This ensures that only one thread (the terminal acting as a consumer) ever accesses the GUI.
#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => ({
        $crate::print_to_stdout_args(format_args!($($arg)*));
    });
}

/// Maps the child application's task ID to its parent terminal print_producer to track parent-child relationships between
/// applications so that applications can print to the correct terminal
static TERMINAL_PRINT_PRODUCERS: Mutex<BTreeMap<usize, DFQueueProducer<Event>>> = Mutex::new(BTreeMap::new());

/// Adds the (child application's task ID, parent terminal print_producer) key-val pair to the map 
/// Simulates connecting an output stream to the application
pub fn add_child(child_task_id: usize, print_producer: DFQueueProducer<Event>) -> Result<(), &'static str> {
    TERMINAL_PRINT_PRODUCERS.lock().insert(child_task_id, print_producer);
    Ok(())
}

/// Removes the (child application's task ID, parent terminal print_producer) key-val pair from the map
/// Called right after an application exits
pub fn remove_child(child_task_id: usize) -> Result<(), &'static str> {
   TERMINAL_PRINT_PRODUCERS.lock().remove(&child_task_id);
   Ok(()) 
}



use core::fmt;
/// Converts the given `core::fmt::Arguments` to a `String` and enqueues the string into the correct terminal print-producer
pub fn print_to_stdout_args(fmt_args: fmt::Arguments) {
    let task_id = match task::get_my_current_task_id() {
        Some(task_id) => {task_id},
        None => {
            // We cannot use log macros here, because when they're mirrored to the vga, they will cause infinite loops on an error.
            // Instead, we write direclty to the serial port. 
            let _ = logger::write_str("\x1b[31m [E] error in print!/println! macro: failed to get current task id \x1b[0m\n");
            return;
        }
    };
    
    // Obtains the correct terminal print producer and enqueues the print event, which will later be popped off
    // and handled by the infinite terminal instance loop 
    let print_map = TERMINAL_PRINT_PRODUCERS.lock();
    let result = print_map.get(&task_id);
    if let Some(selected_term_producer) = result {
        selected_term_producer.enqueue(Event::new_output_event(format!("{}", fmt_args)));
    }
}
