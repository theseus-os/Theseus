#![no_std]
#![feature(alloc)]
extern crate keycodes_ascii;
extern crate spin;
extern crate dfqueue;
extern crate atomic_linked_list; 
extern crate mod_mgmt;
extern crate spawn;
extern crate task;
extern crate memory;
// temporary, should remove this once we fix crate system
extern crate console_types; 
extern crate terminal;

#[macro_use] extern crate lazy_static;
#[macro_use] extern crate alloc;

use console_types::{ConsoleEvent};
use keycodes_ascii::Keycode;
use alloc::string::ToString;
use alloc::arc::Arc;
use alloc::btree_map::BTreeMap;
use spin::Mutex;
use core::sync::atomic::{AtomicUsize, Ordering};
use dfqueue::{DFQueue, DFQueueConsumer, DFQueueProducer};

// / Calls `print!()` with an extra newilne ('\n') appended to the end. 
#[macro_export]
macro_rules! println {
    ($fmt:expr) => (print!(concat!($fmt, "\n")));
    ($fmt:expr, $($arg:tt)*) => (print!(concat!($fmt, "\n"), $($arg)*));

}

/// The main printing macro, which simply pushes an output event to the console's event queue. 
/// This ensures that only one thread (the console acting as a consumer) ever accesses the GUI.
#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => ({
        $crate::print_to_console_args(format_args!($($arg)*));
    });
}


lazy_static! {
    /// Global map that maps the terminal reference number to a producer for its input queue
    /// More info about terminal queues located in the Temrinal crate
    static ref TERMINAL_INPUT_PRODUCERS: Arc<Mutex<BTreeMap<usize, DFQueueProducer<ConsoleEvent>>>> = Arc::new(Mutex::new(BTreeMap::new()));
    // Tracks which terminal is currently being focused on
    static ref CURRENT_TERMINAL_NUM: AtomicUsize  = AtomicUsize::new(1);
}

use core::fmt;
/// Converts the given `core::fmt::Arguments` to a `String` and queues it up to be printed out to the console.
pub fn print_to_console_args(fmt_args: fmt::Arguments) {
    let num = CURRENT_TERMINAL_NUM.load(Ordering::SeqCst);
    // Passes the current terminal number (the one being focused on) to whoever is printing so it knows whether or not to refresh its display
    let _result = terminal::print_to_console(format!("{}", fmt_args), num);
}


// Defines the max number of terminals that can be running 
const MAX_TERMS: usize = 9;


/// Initializes the console by spawning a new thread to handle all console events, and creates a new event queue. 
/// This event queue's consumer is given to that console thread, and a producer reference to that queue is returned. 
/// This allows other modules to push console events onto the queue. 
pub fn init() -> Result<DFQueueProducer<ConsoleEvent>, &'static str> {
    let event_handling_queue: DFQueue<ConsoleEvent> = DFQueue::new();
    let event_handling_consumer = event_handling_queue.into_consumer();
    let returned_producer = event_handling_consumer.obtain_producer();
    // Initializes the default kernel terminal
    let kernel_producer = terminal::Terminal::init(1)?;
    TERMINAL_INPUT_PRODUCERS.lock().insert(1, kernel_producer);
    // Adds this default kernel terminal to the static list of running terminals
    // Note that the list owns all the terminals that are spawned
    spawn::spawn_kthread(input_event_loop, event_handling_consumer, "main input event handling loop".to_string(), None)?;
    Ok(returned_producer)
}

/// Main infinite loop that handles DFQueue input and output events
fn input_event_loop(consumer: DFQueueConsumer<ConsoleEvent>) -> Result<(), &'static str> {
    // variable to track which terminal the user is currently focused on
    // terminal objects have a field term_ref that can be used for this purpose
    let mut num_running: usize = 1;
    // Bool prevents keypresses like ctrl+t from actually being pushed to the terminal scrollback buffer
    let mut meta_keypress = false;
    loop {
        meta_keypress = false;
        use core::ops::Deref;

        // Pops events off the keyboard queue and redirects to the appropriate terminal input queue producer
        let event = match consumer.peek() {
            Some(ev) => ev,
            _ => { continue; }
        };
        match event.deref() {
            &ConsoleEvent::ExitEvent => {
                return Ok(());
            }

            &ConsoleEvent::InputEvent(ref input_event) => {
                let key_input = input_event.key_event;
                // Ctrl + T makes a new terminal tab
                if key_input.modifiers.control && key_input.keycode == Keycode::T && num_running < MAX_TERMS {
                    num_running += 1;
                    // Switches focus to this terminal
                    CURRENT_TERMINAL_NUM.store(num_running, Ordering::SeqCst);
                    let terminal_producer = terminal::Terminal::init(num_running)?;
                    TERMINAL_INPUT_PRODUCERS.lock().insert(num_running, terminal_producer);
                    meta_keypress = true;
                    event.mark_completed();
                }
                // Ctrl + num switches between existing terminal tabs
                if key_input.modifiers.control && (
                    key_input.keycode == Keycode::Num1 ||
                    key_input.keycode == Keycode::Num2 ||
                    key_input.keycode == Keycode::Num3 ||
                    key_input.keycode == Keycode::Num4 ||
                    key_input.keycode == Keycode::Num5 ||
                    key_input.keycode == Keycode::Num6 ||
                    key_input.keycode == Keycode::Num7 ||
                    key_input.keycode == Keycode::Num8 ||
                    key_input.keycode == Keycode::Num9 ) {
                    let selected_num;
                    match key_input.keycode.to_ascii(key_input.modifiers) {
                        Some(key) => {
                            match key.to_digit(10) {
                                Some(digit) => {
                                    selected_num = digit;
                                },
                                None => {
                                    continue;
                                }
                            }
                        },
                        None => {
                            continue;
                        },
                    }
                    // Prevents user from switching to terminal tab that doesn't yet exist
                    if selected_num > num_running as u32 { // does nothing
                    } else {
                        CURRENT_TERMINAL_NUM.store(selected_num as usize, Ordering::SeqCst);
                    }
                    event.mark_completed();
                    meta_keypress = true;
                }

            }
            _ => { }
        }

        // If the keyevent was not for control of the terminal windows
        if !meta_keypress {
            // Clones the input keypress event
            let console_event = event.deref().clone();
            let terminal_input_producers_lock = TERMINAL_INPUT_PRODUCERS.lock(); 
            // Gets the input event producer for the terminal that's currently being focused on
            let current_terminal_num = CURRENT_TERMINAL_NUM.load(Ordering::SeqCst);
            let result = terminal_input_producers_lock.get(&current_terminal_num);
            if let Some(term_input_producer) = result {
                // Enqueues the copied input key event as well as a display event to signal 
                // that the terminal should refresh and display to the vga buffer
                term_input_producer.enqueue(console_event);
                event.mark_completed();
            }
        }
    }
}






