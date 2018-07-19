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
extern crate terminal;
extern crate alloc;

#[macro_use] extern crate log;

use input_event_types::{Event};
use keycodes_ascii::{Keycode, KeyAction};
use dfqueue::{DFQueue, DFQueueConsumer, DFQueueProducer};
use window_manager::GAP_SIZE;
use alloc::string::{String,ToString};




pub fn init() -> Result<DFQueueProducer<Event>, &'static str> {
    let keyboard_event_handling_queue: DFQueue<Event> = DFQueue::new();
    let keyboard_event_handling_consumer = keyboard_event_handling_queue.into_consumer();
    let returned_keyboard_producer = keyboard_event_handling_consumer.obtain_producer();
    // Initializes the default kernel terminal
    let window_object = match window_manager::get_window_obj(GAP_SIZE, GAP_SIZE, frame_buffer::FRAME_BUFFER_WIDTH - GAP_SIZE * 2, frame_buffer::FRAME_BUFFER_HEIGHT - GAP_SIZE * 2 ) {
        Ok(obj) => obj,
        Err(some_err) => {return Err("could not initialize first window object");}
    };

    terminal::Terminal::init(window_object, 0)?; // spawns the default terminal
    spawn::spawn_kthread(input_event_loop, keyboard_event_handling_consumer, "main input event handling loop".to_string(), None)?;
    Ok(returned_keyboard_producer)
}


fn input_event_loop(consumer:DFQueueConsumer<Event>) -> Result<(), &'static str> {
    
    let mut terminal_id_counter: usize = 1; // fix: will not correspond to the number running in the future
    // Bool prevents keypresses like ctrl+t from actually being pushed to the terminal scrollback buffer
    loop {
        let mut meta_keypress = false;
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
                // Creates new terminal window
                if key_input.modifiers.control && key_input.keycode == Keycode::T {
                    if let Some((height_index, window_width, window_height)) = window_manager::adjust_windows() {
                        debug!("starting initialization...,height_idx, window_width, window_height are {}, {}, {}\n ", height_index, window_width, window_height);
                        let window_object = match window_manager::get_window_obj(GAP_SIZE, height_index, window_width, window_height) {
                            Ok(obj) => obj,
                            Err(some_err) => {
                                return Err(some_err);
                            },
                        };
                        debug!("initialization sucessful...");
                        terminal::Terminal::init(window_object, terminal_id_counter)?;
                        terminal_id_counter += 1;
                        meta_keypress = true;
                        event.mark_completed();
                    } else {
                        return Err ("could not create new window");
                    }                    
                }
                if key_input.modifiers.alt && key_input.keycode == Keycode::Tab {
                    window_manager::window_switch()?;
                    meta_keypress = true;
                    event.mark_completed();

                }

                if key_input.modifiers.control && key_input.keycode == Keycode::W && key_input.action == KeyAction::Pressed {
                    debug!("CONTROL W PRESSED");
                    window_manager::delete_active_window();
                    meta_keypress = true;
                    event.mark_completed();
                }
            }
            _ => { }
        }

        // If the keyevent was not for control of the terminal windows
        if !meta_keypress {
            window_manager::put_key_code(event.deref().clone())?;
            event.mark_completed();

        }
    }    
}