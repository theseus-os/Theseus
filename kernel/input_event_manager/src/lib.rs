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
extern crate spawn;
extern crate task;
extern crate mod_mgmt;
extern crate event_types; 
extern crate window_manager;
extern crate path;

#[macro_use] extern crate alloc;
#[macro_use] extern crate log;

use alloc::{
    string::{String, ToString},
    vec::Vec,
    sync::Arc,
};
use event_types::{Event};
use keycodes_ascii::{Keycode, KeyAction};
use dfqueue::{DFQueue, DFQueueConsumer, DFQueueProducer};
use mod_mgmt::{
    CrateNamespace,
    NamespaceDir,
    metadata::CrateType,
};
use spawn::{KernelTaskBuilder, ApplicationTaskBuilder};
use path::Path;

/// Initializes the keyinput queue and the default display
pub fn init() -> Result<DFQueueProducer<Event>, &'static str> {
    // keyinput queue initialization
    let keyboard_event_handling_queue: DFQueue<Event> = DFQueue::new();
    let keyboard_event_handling_consumer = keyboard_event_handling_queue.into_consumer();
    let returned_keyboard_producer = keyboard_event_handling_consumer.obtain_producer();

    // Create the first application CrateNamespace via the following steps:
    // (1) get the default kernel CrateNamespace, which will serve as the new app namespace's recursive namespace,
    // (2) get the directory where the default app namespace should have been populated when mod_mgmt was init'd,
    // (3) create the actual new application CrateNamespace
    let default_kernel_namespace = mod_mgmt::get_default_namespace()
        .ok_or("input_event_manager::init(): default CrateNamespace not yet initialized")?;
    let default_app_namespace_name = CrateType::Application.namespace_name().to_string(); // this will be "_applications"
    let default_app_namespace_dir = mod_mgmt::get_namespaces_directory()
        .and_then(|ns_dir| ns_dir.lock().get_dir(&default_app_namespace_name))
        .ok_or("Couldn't find the directory for the default application CrateNamespace")?;
    let default_app_namespace = Arc::new(CrateNamespace::new(
        default_app_namespace_name,
        NamespaceDir::new(default_app_namespace_dir),
        Some(default_kernel_namespace.clone()),
    ));

    let terminal_print_path = default_app_namespace.get_crate_file_starting_with("terminal_print-")
        .ok_or("Couldn't find terminal_print application in default app namespace")?;
    let shell_path = default_app_namespace.get_crate_file_starting_with("shell-")
        .ok_or("Couldn't find terminal application in default app namespace")?;
    let app_io_path = default_app_namespace.get_crate_file_starting_with("app_io-")
        .ok_or("Couldn't find terminal application in default app namespace")?;

    // Spawns the terminal print crate so that we can print to the terminal
    ApplicationTaskBuilder::new(terminal_print_path)
        .name("terminal_print_singleton".to_string())
        .namespace(default_app_namespace.clone())
        .singleton()
        .spawn()?;

    ApplicationTaskBuilder::new(app_io_path)
        .name("application_io_manager".to_string())
        .singleton()
        .spawn()?;

    // Spawn the default terminal (will also start the windowing manager)
    ApplicationTaskBuilder::new(shell_path)
        .name("default_terminal".to_string())
        .namespace(default_app_namespace)
        .spawn()?;

    // start the input event loop thread
    KernelTaskBuilder::new(input_event_loop, keyboard_event_handling_consumer)
        .name("input_event_loop".to_string())
        .pin_on_core(0)
        .spawn()?;

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
                    let args: Vec<String> = vec![]; // terminal::main() does not accept any arguments
                    ApplicationTaskBuilder::new(Path::new(String::from("shell")))
                        .argument(args)
                        .pin_on_core(0)
                        .name(task_name)
                        .spawn()?;
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
                    window_manager::send_event_to_active(Event::ExitEvent)?; // tells application to exit
                }
            }
            _ => { }
        }

        // If the keyevent was not for control of the terminal windows, enqueues keycode into active window
        if !meta_keypress {
            window_manager::send_event_to_active(event.deref().clone())?;
            event.mark_completed();

        }
    }    
}