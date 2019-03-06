#![no_std]
#![feature(alloc)]
//! This crate contains the implementation of the special root directory. The only way that this 
//! directory implementation differs from VFSDirectory is that there is no parent field (becuase the 
//! root has no parent directory), and that internal calls to parent will return some type of error value

#[macro_use] extern crate log;
#[macro_use] extern crate alloc;
#[macro_use] extern crate lazy_static;
extern crate spin;
extern crate fs_node;

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use alloc::boxed::Box;
use spin::Mutex;
use alloc::sync::Arc;
use alloc::collections::BTreeMap;
use fs_node::{DirRef, Directory, FileOrDir, FsNode};


pub const ROOT_DIRECTORY_NAME: &'static str = "";

lazy_static! {
    /// The root directory
    /// Returns a tuple for easy access to the name of the root so we don't have to lock it
    pub static ref ROOT: (String, DirRef) = {
        let root_dir = RootDirectory {
            name: ROOT_DIRECTORY_NAME.to_string(),
            children: BTreeMap::new() 
        };

        let strong_root = Arc::new(Mutex::new(Box::new(root_dir) as Box<Directory + Send>));
    
        (ROOT_DIRECTORY_NAME.to_string(), strong_root)

    };
}

/// Returns a reference to the root directory.
pub fn get_root() -> &'static DirRef {
    &ROOT.1
}

/// A struct that represents a node in the VFS 
pub struct RootDirectory {
    /// The name of the directory
    name: String,
    /// A list of DirRefs or pointers to the child directories   
    children: BTreeMap<String, FileOrDir>,
}

impl Directory for RootDirectory {
    fn insert_child(&mut self, child: FileOrDir) -> Result<Option<FileOrDir>, &'static str> {
        // gets the name of the child node to be added
        let name = child.get_name();
        // inserts new child, if that child already exists the old value is returned
        Ok(self.children.insert(name, child))
    }

    fn get_child(&self, child_name: &str) -> Option<FileOrDir> {
        self.children.get(child_name).cloned()
    }

    /// Returns a string listing all the children in the directory
    fn list_children(&mut self) -> Vec<String> {
        self.children.keys().cloned().collect()
    }
}

impl FsNode for RootDirectory {
    /// Recursively gets the absolute pathname as a String
    fn get_path_as_string(&self) -> String {
        format!("{}/", ROOT_DIRECTORY_NAME.to_string()).to_string()
    }

    fn get_name(&self) -> String {
        ROOT_DIRECTORY_NAME.to_string()
    }

    /// we just return the root itself because it is the top of the filesystem
    fn get_parent_dir(&self) -> Result<DirRef, &'static str> {
        Ok(get_root().clone())
    }
}
