#![no_std]
#![feature(alloc)]

#[macro_use] extern crate alloc;
extern crate fs_node;

use alloc::String;
use alloc::arc::Arc;
use fs_node::{StrongAnyDirRef, FileDirectory};

/// A structure that contains Environmnt variables for a given task
pub struct Environment {
    /// The working directory for given tasks
    pub working_dir: StrongAnyDirRef, 
}

impl Environment {
    /// Gets the absolute file path of the working directory
    pub fn get_wd_path(&self) -> String {
        let wd = self.working_dir.lock();
        wd.get_path_as_string()
    }

    /// Sets working directory
    pub fn set_wd(&mut self, new_dir: StrongAnyDirRef) {
        self.working_dir = Arc::clone(&new_dir);
    }

}