#![no_std]
#![feature(alloc)]

#[macro_use] extern crate alloc;
extern crate vfs;

use alloc::String;
use alloc::arc::Arc;
use vfs::{StrongDirRef, FileDirectory, VFSDirectory};

/// A structure that contains Environmnt variables for a given task
pub struct Environment {
    /// The working directory for given tasks
    pub working_dir: StrongDirRef<VFSDirectory>, 
}

impl Environment {
    /// Gets the absolute file path of the working directory
    pub fn get_wd_path(&self) -> String {
        let wd = self.working_dir.lock();
        wd.get_path_as_string()
    }

    /// Sets working directory
    pub fn set_wd(&mut self, new_dir: StrongDirRef<VFSDirectory>) {
        self.working_dir = Arc::clone(&new_dir);
    }

}