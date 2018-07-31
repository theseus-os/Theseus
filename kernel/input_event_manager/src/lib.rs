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
extern crate input_event_types; 
extern crate frame_buffer;
extern crate window_manager;

#[macro_use] extern crate alloc;
#[macro_use] extern crate log;

use input_event_types::{Event};
use keycodes_ascii::{Keycode, KeyAction};
use dfqueue::{DFQueue, DFQueueConsumer, DFQueueProducer};
use window_manager::GAP_SIZE;
use alloc::string::{String,ToString};



/// Initializes the keyinput queue and the default display
pub fn init() -> Result<DFQueueProducer<Event>, &'static str> {
    /// keyinput queue initialization
    let keyboard_event_handling_queue: DFQueue<Event> = DFQueue::new();
    let keyboard_event_handling_consumer = keyboard_event_handling_queue.into_consumer();
    let returned_keyboard_producer = keyboard_event_handling_consumer.obtain_producer();
    // Initializes the default window object (will also start the windowing manager)
    let term_module = memory::get_module("__a_terminal").ok_or("Error: terminal module not found")?;
    let terminal_id_counter = 0;
    let term_num = terminal_id_counter.to_string();
    // passes the terminal reference number in the form of the Vec<String>. The terminal::init() function will parse it into a usize
    // this is a temporary hack to get around for the argument type requirements for applications
    let args = vec![term_num]; 
    spawn::spawn_application(term_module, args, Some("default_terminal".to_string()), None)?; // spawns the default terminal
    spawn::spawn_kthread(input_event_loop, keyboard_event_handling_consumer, "main input event handling loop".to_string(), None)?;
    Ok(returned_keyboard_producer)
}

/// Handles all key inputs to the system
fn input_event_loop(consumer:DFQueueConsumer<Event>) -> Result<(), &'static str> {
    let mut terminal_id_counter: usize = 1; 
    loop {
        let mut meta_keypress = false; // bool prevents keypresses to control the terminals themselves from getting logged to the active terminal
        use core::ops::Deref;   

        // Pops events off the keyboard queue and redirects to the appropriate terminal input queue producer
        let event = match consumer.peek() {
            Some(ev) => ev,
            _ => { continue; }
        };
        match event.deref() {
            &Event::ExitEvent => {
                trace!("exiting the main loop of the input event manager");
                return Ok(()); 
            }

            &Event::InputEvent(ref input_event) => {
                let key_input = input_event.key_event;
                // The following are keypresses for control over the windowing system
                // Creates new terminal window
                if key_input.modifiers.control && key_input.keycode == Keycode::T && key_input.action == KeyAction::Pressed {
                    let task_name: String = format!("terminal {}", terminal_id_counter);
                    let term_num = terminal_id_counter.to_string();
                    // passes the terminal reference number in the form of the Vec<String>. The terminal::init() function will parse it into a usize
                    // this is a temporary hack to get around for the argument type requirements for applications
                    let args = vec![term_num]; 
                    let term_module = memory::get_module("__a_terminal").ok_or("Error: terminal module not found")?;
                    spawn::spawn_application(term_module, args, Some(task_name), None)?;
                    terminal_id_counter += 1;
                    meta_keypress = true;
                    event.mark_completed();
                  
                }

                // Switches between terminal windows
                if key_input.modifiers.alt && key_input.keycode == Keycode::Tab && key_input.action == KeyAction::Pressed {
                    window_manager::switch()?;
                    meta_keypress = true;
                    event.mark_completed();

                }

                // Deletes the active window (whichever window Ctrl + W is logged in)
                if key_input.modifiers.control && key_input.keycode == Keycode::W && key_input.action == KeyAction::Pressed {
                    window_manager::delete_active_window();
                    meta_keypress = true;
                    event.mark_completed();
                }
            }
            _ => { }
        }

        // If the keyevent was not for control of the terminal windows, enqueues keycode into active window
        if !meta_keypress {
            window_manager::put_key_code(event.deref().clone())?;
            event.mark_completed();

        }
    }    
}