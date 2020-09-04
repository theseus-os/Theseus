//! This crate contains a simple routine to start the first application (or set of applications). 
//! 
//! This should be invoked at or towards the end of the kernel initialization procedure. 

#![no_std]

extern crate alloc;
#[macro_use] extern crate log;
extern crate spawn;
extern crate mod_mgmt;
extern crate path;

use alloc::string::ToString;
use mod_mgmt::CrateNamespace;
use path::Path;

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
    let (shell_file, _ns) = CrateNamespace::get_crate_object_file_starting_with(&new_app_ns, "shell-")
        .ok_or("Couldn't find shell application in default app namespace")?;

    let path = Path::new(shell_file.lock().get_absolute_path());
    info!("Starting first application: crate at {:?}", path);
    // Spawn the default shell
    spawn::new_application_task_builder(path, Some(new_app_ns))?
        .name("default_shell".to_string())
        .spawn()?;

    {
        let new_app_ns = mod_mgmt::create_application_namespace(None)?;
        let (rq_eval_file, _ns) = CrateNamespace::get_crate_object_file_starting_with(&new_app_ns, "bm-")
            .ok_or("Couldn't find bm application in default app namespace")?;

        let path = Path::new(rq_eval_file.lock().get_absolute_path());
        info!("Starting bm application: crate at {:?}", path);
        // Spawn the default shell
        spawn::new_application_task_builder(path, Some(new_app_ns))?
            .name("bm_app".to_string())
            .spawn()?;
    }

    Ok(())
}
