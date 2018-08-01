//! Applications that want to print to their parent terminal must import this crate
//! To print, applications can call the println! or print! macros
//! 
//! Printing simulates the parent-child relationship for standard out when parent applications spawn child applications
//! For example, when a terminal runs a command, it will map that child command's task ID to that terminal's print producer
//! so that any output from the child command is identified and outputted to the parent terminal
//! 
//! *Note: this printing crate only supports single-task child applications

#![no_std]
#![feature(alloc)]

#[macro_use] extern crate alloc;
#[macro_use] extern crate log;
#[macro_use] extern crate lazy_static;
extern crate task;
extern crate dfqueue;
extern crate event_types;
extern crate spin;

use event_types::Event;
use dfqueue::DFQueueProducer;
use alloc::arc::Arc;
use alloc::btree_map::BTreeMap;
use spin::Mutex;

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

/// Maps the child application's task ID to its parent terminal print_producer to track parent-child relationships between
/// applications so that applications can print to the correct terminal
lazy_static! {
    static ref TERMINAL_PRINT_PRODUCERS: Arc<Mutex<BTreeMap<usize, DFQueueProducer<Event>>>> = Arc::new(Mutex::new(BTreeMap::new()));
}

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
        None => {error!("could not get current task id" ); return}
    };
    
    // Obtains the correct temrinal print producer and enqueues the print event, which will later be popped off
    // and handled by the infinite temrinal instance loop 
    let print_map = TERMINAL_PRINT_PRODUCERS.lock();
    let result = print_map.get(&task_id);
    if let Some(selected_term_producer) = result {
        selected_term_producer.enqueue(Event::new_output_event(format!("{}", fmt_args)));
    }
}