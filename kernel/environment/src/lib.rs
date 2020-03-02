#![no_std]

extern crate alloc;
extern crate fs_node;
extern crate root;

use alloc::{
    string::String,
    sync::Arc,
};
use fs_node::DirRef;

/// A structure that contains environment state for a given `Task` or group of `Task`s.
/// 
/// A default environment can be created with the following state:
/// * The working directory is the `root` directory.
///
pub struct Environment {
    /// The "current working directory", i.e., 
    /// where a task's relative path begins upon first execution.
    pub working_dir: DirRef, 
}

impl Environment {
    /// Gets the absolute file path of the working directory
    pub fn get_wd_path(&self) -> String {
        let wd = self.working_dir.lock();
        wd.get_absolute_path()
    }
}

impl Default for Environment {
    fn default() -> Environment {
        Environment {
            working_dir: Arc::clone(root::get_root()),
        }
    }
}