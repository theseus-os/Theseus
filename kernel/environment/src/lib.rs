#![no_std]
#![feature(alloc)]

extern crate alloc;
extern crate fs_node;

use alloc::string::String;
use fs_node::DirRef;

/// A structure that contains Environmnt variables for a given task
/// For now, the one variable is the current working directory of the task, which is 
/// stored as a strong pointer to a directory within the filesystem
pub struct Environment {
    /// The working directory for given tasks
    pub working_dir: DirRef, 
}

impl Environment {
    /// Gets the absolute file path of the working directory
    pub fn get_wd_path(&self) -> String {
        let wd = self.working_dir.lock();
        wd.get_path_as_string()
    }
}