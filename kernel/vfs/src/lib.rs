//! File system abstractions for Theseus.
//!
//! These are expected to eventually replace `fs_node`.
//!
//! Theseus uses forest mounting to keep track of different file systems. The
//! differences between forest mounting and tree mounting are explained below:
//!
//! - Forest mounting (i.e. Windows flavour): There are multiple directory tree
//!   structures, with each one being a contained file system. For example,
//!   Windows' drives (e.g. `C:`). Technically Windows does support folder mount
//!   points but we're ignoring that for the sake of simplicity.
//! - Tree mounting (i.e. Unix flavour): There is a single directory tree
//!   structure originiating in the root directory. Filesystems are mounted to
//!   subdirectories of the root.
//!
//! Theseus file systems are similar to Windows drives, except they use an
//! arbitrary string identifier rather than a letter.

#![cfg_attr(not(test), no_std)]
#![feature(trait_upcasting)]
#![allow(incomplete_features)]

extern crate alloc;

use alloc::{string::String, sync::Arc, vec::Vec};
use spin::RwLock;

mod node;
mod path;

pub use path::*;
pub use node::*;

/// The currently mounted file systems.
///
/// We use a [`Vec`] rather than a [`HashMap`] because it's more performant when
/// the number of entries is in the tens. It also avoids the indirection of
/// `lazy_static`.
static FILE_SYSTEMS: RwLock<Vec<(String, Arc<dyn FileSystem>)>> = RwLock::new(Vec::new());

/// A file system.
pub trait FileSystem: Send + Sync {
    /// Returns the root directory.
    fn root(&self) -> Arc<dyn Directory>;
}

/// Returns the file system with the specified `id`.
pub fn file_system(id: &str) -> Option<Arc<dyn FileSystem>> {
    FILE_SYSTEMS.read().iter().find(|s| s.0 == id).map(|(_, fs)| fs).cloned()
}

/// Mounts a file system.
///
/// # Errors
///
/// Returns an error if a file system with the specified `id` already exists.
#[allow(clippy::result_unit_err)]
pub fn mount_file_system(id: String, fs: Arc<dyn FileSystem>) -> Result<(), ()> {
    let mut file_systems = FILE_SYSTEMS.write();
    if file_systems.iter().any(|s| s.0 == id) {
        return Err(());
    };
    file_systems.push((id, fs));
    Ok(())
}

/// Unmounts the file system with the specified `id`.
///
/// Returns the file system if it exists.
pub fn unmount_file_system(id: &str) -> Option<Arc<dyn FileSystem>> {
    let mut file_systems = FILE_SYSTEMS.write();
    let index = file_systems.iter().position(|s| s.0 == id)?;
    Some(file_systems.remove(index).1)
}
