#![no_std]
#![feature(alloc)]

#[macro_use] extern crate alloc;
extern crate vfs;

use alloc::String;
use alloc::arc::Arc;
use vfs::StrongDirRef;

pub struct Environment {
    pub working_dir: StrongDirRef
}

impl Environment {
    pub fn get_wd_path(&self) -> String {
        let wd = self.working_dir.lock();
        wd.basename.clone()
    }

    /// Sets working directory
    pub fn set_wd(&mut self, new_dir: StrongDirRef) {
        self.working_dir = Arc::clone(&new_dir);
    }
    
    /// Looks for the child directory specified by dirname and sets it as the current directory
    pub fn set_chdir_as_wd(&mut self, dirname: String) -> Result<(), &'static str> {
        let wd = match self.working_dir.lock().get_child_dir(dirname).clone() {
            Some(dir) => dir,
            None => {
                return Err("no such directory");
            }
        };
        self.working_dir = wd;
        return Ok(());
    }
}