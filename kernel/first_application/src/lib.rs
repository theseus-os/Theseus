//! This crate contains a simple routine to start the first application (or set of applications). 
//! 
//! This should be invoked at or towards the end of the kernel initialization procedure. 
//!
//! ## Important Dependency Note
//!  
//! In general, Theseus kernel crates *cannot* depend on application crates.
//! However, this crate is a special exception in that it directly loads and runs
//! the first application crate.
//! 
//! Thus, it's safest to ensure that first application crate is always included
//! in the build by specifying it as a direct dependency here.
//! 
//! Currently, that crate is `applications/shell`, but if it changes,
//! we should change that dependendency in this crates `Cargo.toml` manifest.

#![no_std]

extern crate alloc;
#[macro_use] extern crate log;
extern crate spawn;
extern crate mod_mgmt;
extern crate path;

use alloc::string::ToString;
use mod_mgmt::CrateNamespace;
use path::Path;

/// See the crate-level docs and this crate's `Cargo.toml` for more.
const FIRST_APPLICATION_CRATE_NAME: &str = "shell-";

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

    // NOTE: see crate-level docs and note in this crate's `Cargo.toml`.
    let (app_file, _ns) = CrateNamespace::get_crate_object_file_starting_with(
        &new_app_ns, 
        FIRST_APPLICATION_CRATE_NAME,
    ).ok_or("Couldn't find first application in default app namespace")?;

    let path = Path::new(app_file.lock().get_absolute_path());
    info!("Starting first application: crate at {:?}", path);
    // Spawn the default shell
    spawn::new_application_task_builder(path, Some(new_app_ns))?
        .name("default_shell".to_string())
        .spawn()?;

    Ok(())
}
