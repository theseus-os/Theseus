//! Input event manager responsible for handling user input into Theseus
//! 
//! Input event manager spawns a default terminal 
//! Currently, this default terminal cannot be closed because it is the log point for all messages from the kernel
//! 
//! Input event manager takes keyinputs from the keyboard crate, handles any meta-keypresses (i.e. those for
//! controlling the applications themselves) and passes ordinary keypresses to the window manager
//! In the future, the input event manager will handle other forms of input to the OS

#![no_std]
#[macro_use] extern crate log;
extern crate keycodes_ascii;
extern crate spin;
extern crate dfqueue;
extern crate spawn;
extern crate task;
extern crate mod_mgmt;
extern crate event_types; 
extern crate window_manager;
extern crate path;
extern crate alloc;
extern crate window_manager_alpha;
extern crate frame_buffer_alpha;

use alloc::{
    string::ToString,
    sync::Arc,
};
use event_types::{Event};
use dfqueue::{DFQueue, DFQueueProducer};
use mod_mgmt::{
    CrateNamespace,
    NamespaceDir,
    metadata::CrateType,
};
use spawn::ApplicationTaskBuilder;


/// Initializes the keyinput queue and the default display
pub fn init() -> Result<(DFQueueProducer<Event>, DFQueueProducer<Event>), &'static str> {
    // keyinput queue initialization
    let keyboard_event_handling_queue: DFQueue<Event> = DFQueue::new();
    let keyboard_event_handling_consumer = keyboard_event_handling_queue.into_consumer();
    let returned_keyboard_producer = keyboard_event_handling_consumer.obtain_producer();

    // mouse input queue initialization
    let mouse_event_handling_queue: DFQueue<Event> = DFQueue::new();
    let mouse_event_handling_consumer = mouse_event_handling_queue.into_consumer();
    let returned_mouse_producer = mouse_event_handling_consumer.obtain_producer();

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

    // init frame_buffer_alpha
    let rs = frame_buffer_alpha::init();
    let final_fb = match rs {
        Ok(final_fb) => {
            trace!("Frame_buffer_alpha initialized successfully.");
            final_fb
        }
        Err(err) => { 
            error!("captain::init(): failed to initialize frame_buffer_alpha");
            return Err(err);
        }
    };

    // init window manager_alpha
    let rs = window_manager_alpha::init(keyboard_event_handling_consumer, mouse_event_handling_consumer, final_fb);
    match rs {
        Ok(_) => {
            trace!("window manager alpha initialized successfully.");
        }
        Err(err) => { 
            error!("captain::init(): failed to initialize window manager alpha");
            return Err(err);
        }
    }

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

    Ok((returned_keyboard_producer, returned_mouse_producer))
}
