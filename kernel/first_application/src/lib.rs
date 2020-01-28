//! This crate contains a simple routine to start the first application (or set of applications). 
//! 
//! This should be invoked at or towards the end of the kernel initialization procedure. 

#![no_std]

extern crate alloc;
extern crate spawn;
extern crate mod_mgmt;

use alloc::string::ToString;
use spawn::ApplicationTaskBuilder;

/// Starts the first applications that run in Theseus 
/// by creating a new "default" application namespace
/// and spawning the first application `Task`(s). 
/// 
/// Currently this only spawns a shell (terminal),
/// but in the future it could spawn a fuller desktop environment. 
/// 
/// Kernel initialization routines should be complete before invoking this. 
pub fn start() -> Result<(), &'static str> {
    let new_app_ns = mod_mgmt::create_application_namespace(None)?;
    let app_io_path = new_app_ns.get_crate_file_starting_with("app_io-")
        .ok_or("Couldn't find app_io application in default app namespace")?;
    let shell_path = new_app_ns.get_crate_file_starting_with("shell-")
        .ok_or("Couldn't find shell application in default app namespace")?;

    ApplicationTaskBuilder::new(app_io_path)
        .name("application_io_manager".to_string())
        // .namespace(new_app_ns)
        .singleton()
        .spawn()?;

    // Spawn the default shell
    ApplicationTaskBuilder::new(shell_path)
        .name("default_shell".to_string())
        .namespace(new_app_ns)
        .spawn()?;

    Ok(())
}
