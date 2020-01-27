//! Input event manager responsible for handling user input into Theseus
//! 
//! Input event manager spawns a default terminal 
//! Currently, this default terminal cannot be closed because it is the log point for all messages from the kernel
//! 
//! Input event manager takes keyinputs from the keyboard crate, handles any meta-keypresses (i.e. those for
//! controlling the applications themselves) and passes ordinary keypresses to the window manager
//! In the future, the input event manager will handle other forms of input to the OS

#![no_std]
extern crate spawn;
extern crate mod_mgmt;
extern crate alloc;

use alloc::string::ToString;
use spawn::ApplicationTaskBuilder;

/// Initializes the i/o related tasks.
pub fn init() -> Result<(), &'static str> {
    let new_app_ns = mod_mgmt::create_application_namespace(None)?;
    let shell_path = new_app_ns.get_crate_file_starting_with("shell-")
        .ok_or("Couldn't find terminal application in default app namespace")?;
    let app_io_path = new_app_ns.get_crate_file_starting_with("app_io-")
        .ok_or("Couldn't find terminal application in default app namespace")?;

    ApplicationTaskBuilder::new(app_io_path)
        .name("application_io_manager".to_string())
        // .namespace(new_app_ns)
        .singleton()
        .spawn()?;

    // Spawn the default terminal
    ApplicationTaskBuilder::new(shell_path)
        .name("default_terminal".to_string())
        .namespace(new_app_ns)
        .spawn()?;

    Ok(())
}
