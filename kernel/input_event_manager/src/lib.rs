//! Input event manager responsible for handling user input into Theseus
//! 
//! Input event manager spawns a default terminal 
//! Currently, this default terminal cannot be closed because it is the log point for all messages from the kernel
//! 
//! Input event manager takes keyinputs from the keyboard crate, handles any meta-keypresses (i.e. those for
//! controlling the applications themselves) and passes ordinary keypresses to the window manager
//! In the future, the input event manager will handle other forms of input to the OS

#![no_std]
extern crate keycodes_ascii;
extern crate spin;
extern crate dfqueue;
extern crate atomic_linked_list; 
extern crate spawn;
extern crate task;
extern crate memory;
// temporary, should remove this once we fix crate system
extern crate event_types; 
extern crate frame_buffer;
extern crate window_manager;
extern crate path;
extern crate alloc;

use event_types::{Event};
use dfqueue::{DFQueue, DFQueueConsumer, DFQueueProducer};
use spawn::{KernelTaskBuilder, ApplicationTaskBuilder};
use path::Path;
use alloc::string::{String, ToString};

/// Initializes the keyinput queue and the default display
pub fn init() -> Result<(DFQueueProducer<Event>, DFQueueConsumer<Event>, DFQueueProducer<Event>, DFQueueConsumer<Event>), &'static str> {
    // keyinput queue initialization
    let keyboard_event_handling_queue: DFQueue<Event> = DFQueue::new();
    let keyboard_event_handling_consumer = keyboard_event_handling_queue.into_consumer();
    let returned_keyboard_producer = keyboard_event_handling_consumer.obtain_producer();

    // mouse input queue initialization
    let mouse_event_handling_queue: DFQueue<Event> = DFQueue::new();
    let mouse_event_handling_consumer = mouse_event_handling_queue.into_consumer();
    let returned_mouse_producer = mouse_event_handling_consumer.obtain_producer();

    // Spawns the terminal print crate so that we can print to the terminal
    ApplicationTaskBuilder::new(Path::new(String::from("terminal_print")))
        .name("terminal_print_singleton".to_string())
        .singleton()
        .spawn()?;

    // Spawn the default terminal (will also start the windowing manager)
    ApplicationTaskBuilder::new(Path::new(String::from("terminal")))
        .name("default_terminal".to_string())
        .spawn()?;

    Ok((returned_keyboard_producer, keyboard_event_handling_consumer, returned_mouse_producer, mouse_event_handling_consumer))
}
