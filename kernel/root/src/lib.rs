#![no_std]
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
use spin::Mutex;
use alloc::sync::{Arc, Weak};
use alloc::collections::BTreeMap;
use fs_node::{DirRef, Directory, FileOrDir, FsNode, WeakDirRef};


pub const ROOT_DIRECTORY_NAME: &str = "";

lazy_static! {
    /// The root directory
    /// Returns a tuple for easy access to the name of the root so we don't have to lock it
    pub static ref ROOT: (String, DirRef) = {
        let root_dir = RootDirectory {
            children: BTreeMap::new() 
        };
        let strong_root = Arc::new(Mutex::new(root_dir)) as DirRef;
        (ROOT_DIRECTORY_NAME.to_string(), strong_root)
    };
}

/// Returns a reference to the root directory.
pub fn get_root() -> &'static DirRef {
    &ROOT.1
}

/// A struct that represents a node in the VFS 
pub struct RootDirectory {
    /// A list of DirRefs or pointers to the child directories   
    children: BTreeMap<String, FileOrDir>,
}

impl Directory for RootDirectory {
    fn insert(&mut self, node: FileOrDir) -> Result<Option<FileOrDir>, &'static str> {
        let name = node.get_name();
        if let Some(mut old_node) = self.children.insert(name, node) {
            old_node.set_parent_dir(Weak::<Mutex<RootDirectory>>::new());
            Ok(Some(old_node))
        } else {
            Ok(None)
        }
    }

    fn get(&self, name: &str) -> Option<FileOrDir> {
        self.children.get(name).cloned()
    }

    fn list(&self) -> Vec<String> {
        self.children.keys().cloned().collect()
    }

    fn remove(&mut self, node: &FileOrDir) -> Option<FileOrDir> {
        // Prevents removal of root
        if let FileOrDir::Dir(dir) = node {
            if Arc::ptr_eq(dir, get_root()) {
                error!("Ignoring attempt to remove the root directory");
                return None;
            }
        }
        
        if let Some(mut old_node) = self.children.remove(&node.get_name()) {
            old_node.set_parent_dir(Weak::<Mutex<RootDirectory>>::new());
            Some(old_node)
        } else {
            None
        }
    }
}

impl FsNode for RootDirectory {
    /// Recursively gets the absolute pathname as a String
    fn get_absolute_path(&self) -> String {
        format!("{ROOT_DIRECTORY_NAME}/")
    }

    fn get_name(&self) -> String {
        ROOT_DIRECTORY_NAME.to_string()
    }

    /// we just return the root itself because it is the top of the filesystem
    fn get_parent_dir(&self) -> Option<DirRef> {
        Some(get_root().clone())
    }

    fn set_parent_dir(&mut self, _: WeakDirRef) {
        // do nothing
    }
}
