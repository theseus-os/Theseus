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
extern crate vga_buffer;
// temporary, should remove this once we fix crate system
extern crate input_event_types; 
extern crate terminal;

#[macro_use] extern crate lazy_static;
#[macro_use] extern crate alloc;

use vga_buffer::VgaBuffer;
use input_event_types::{Event};
use keycodes_ascii::{Keycode, KeyAction};
use alloc::string::ToString;
use alloc::btree_map::BTreeMap;
use core::sync::atomic::{AtomicUsize, Ordering};
use dfqueue::{DFQueue, DFQueueConsumer, DFQueueProducer};

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


lazy_static! {
    // Tracks which terminal is currently being focused on
    static ref CURRENT_TERMINAL_NUM: AtomicUsize  = AtomicUsize::new(0);
}

use core::fmt;
/// Converts the given `core::fmt::Arguments` to a `String` and queues it up to be printed out to the input_event_manager.
pub fn print_to_stdout_args(fmt_args: fmt::Arguments) {
    let num = CURRENT_TERMINAL_NUM.load(Ordering::SeqCst);
    // Passes the current terminal number (the one being focused on) to whoever is printing so it knows whether or not to refresh its display
    let _result = terminal::print_to_stdout(format!("{}", fmt_args), num);
}

// Defines the max number of terminals that can be running 
const MAX_TERMS: usize = 9;

#[derive(Debug)]
//contains the args necessary to pass to input event loop
struct InputEventLoopArgs {
    // consumes events from the keyboard queue
    keyboard_consumer: DFQueueConsumer<Event>,
    // Maps the terminal reference number to its input event queue (more info in the terminal crate)
    terminal_input_producers: BTreeMap<usize, DFQueueProducer<Event>>,

}
/// Initializes the input_event_manager by spawning a new thread to handle all input_event_manager events, and creates a new event queue. 
/// This event queue's consumer is given to that input_event_manager thread, and a producer reference to that queue is returned. 
/// This allows other modules to push input_event_manager events onto the queue. 
pub fn init() -> Result<DFQueueProducer<Event>, &'static str> {
    let keyboard_event_handling_queue: DFQueue<Event> = DFQueue::new();
    let keyboard_event_handling_consumer = keyboard_event_handling_queue.into_consumer();
    let returned_keyboard_producer = keyboard_event_handling_consumer.obtain_producer();
    let vga_buffer = VgaBuffer::new(); // temporary: we intialize a vga buffer to pass the terminal as the text display
    // Initializes the default kernel terminal
    let kernel_producer = terminal::Terminal::init(vga_buffer, 0)?;
    let mut terminal_input_producers = BTreeMap::new();
    // populates a struct with the args needed for input_event_loop
    terminal_input_producers.insert(0, kernel_producer);
    let input_event_loop_args = InputEventLoopArgs {
        keyboard_consumer: keyboard_event_handling_consumer,
        terminal_input_producers: terminal_input_producers,
    };
    // Adds this default kernel terminal to the static list of running terminals
    // Note that the list owns all the terminals that are spawned
    spawn::spawn_kthread(input_event_loop, input_event_loop_args , "main input event handling loop".to_string(), None)?;
    Ok(returned_keyboard_producer)
}

/// Main infinite loop that handles DFQueue input and output events
fn input_event_loop(args: InputEventLoopArgs) -> Result<(), &'static str> {
    // variable to track which terminal the user is currently focused on
    // terminal objects have a field term_ref that can be used for this purpose
    let mut terminal_id_counter: usize = 1;
    let consumer = args.keyboard_consumer;
    let mut terminal_input_producers = args.terminal_input_producers;
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
            &Event::ExitEvent => {
                return Ok(()); 
            }

            &Event::InputEvent(ref input_event) => {
                let key_input = input_event.key_event;
                // Ctrl + T makes a new terminal tab
                if key_input.modifiers.control && key_input.keycode == Keycode::T && key_input.action == KeyAction::Pressed 
                && terminal_id_counter < MAX_TERMS {
                    // Switches focus to this terminal
                    CURRENT_TERMINAL_NUM.store(terminal_id_counter , Ordering::SeqCst); // -1 for 0-indexing
                    let vga_buffer = VgaBuffer::new();
                    let terminal_producer = terminal::Terminal::init(vga_buffer, terminal_id_counter)?;
                    terminal_input_producers.insert(terminal_id_counter , terminal_producer);
                    meta_keypress = true;
                    terminal_id_counter += 1;
                    event.mark_completed();
                }
                // Ctrl + num switches between existing terminal tabs
                if key_input.modifiers.control && key_input.action == KeyAction::Pressed &&(
                    key_input.keycode == Keycode::Num1 ||
                    key_input.keycode == Keycode::Num2 ||
                    key_input.keycode == Keycode::Num3 ||
                    key_input.keycode == Keycode::Num4 ||
                    key_input.keycode == Keycode::Num5 ||
                    key_input.keycode == Keycode::Num6 ||
                    key_input.keycode == Keycode::Num7 ||
                    key_input.keycode == Keycode::Num8 ||
                    key_input.keycode == Keycode::Num9 ) 
                {
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
                    if selected_num > terminal_id_counter as u32 { // does nothing
                    } else { 
                        CURRENT_TERMINAL_NUM.store((selected_num -1) as usize, Ordering::SeqCst); 
                    }
                    event.mark_completed();
                    meta_keypress = true;
                }

                // Cycles forward one terminal
                if key_input.modifiers.control && key_input.keycode == Keycode::PageUp && key_input.action == KeyAction::Pressed {
                    let mut current_num = CURRENT_TERMINAL_NUM.load(Ordering::SeqCst);
                    if current_num  < terminal_id_counter {
                        current_num += 1; 
                        CURRENT_TERMINAL_NUM.store(current_num, Ordering::SeqCst);
                    } else {
                        CURRENT_TERMINAL_NUM.store(0, Ordering::SeqCst);
                    }
                }
                // Cycles backwards one terminl
                if key_input.modifiers.control && key_input.keycode == Keycode::PageDown && key_input.action == KeyAction::Pressed {
                    let mut current_num = CURRENT_TERMINAL_NUM.load(Ordering::SeqCst);
                    if current_num  > 0 {
                        current_num -= 1; 
                        CURRENT_TERMINAL_NUM.store(current_num, Ordering::SeqCst);
                    } else {
                        CURRENT_TERMINAL_NUM.store(terminal_id_counter, Ordering::SeqCst);
                    }
                }

            }
            _ => { }
        }

        // If the keyevent was not for control of the terminal windows
        if !meta_keypress {
            // Clones the input keypress event
            let input_event = event.deref().clone();
            // Gets the input event producer for the terminal that's currently being focused on
            let current_terminal_num = CURRENT_TERMINAL_NUM.load(Ordering::SeqCst);
            let result = terminal_input_producers.get(&current_terminal_num);
            if let Some(term_input_producer) = result {
                // Enqueues the copied input key event as well as a display event to signal 
                // that the terminal should refresh and display to the vga buffer
                term_input_producer.enqueue(input_event);
                event.mark_completed();
            }
        }
    }
}






