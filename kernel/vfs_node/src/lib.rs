#![no_std]

//! This crate contains a very basic, generic concrete implementation of the Directory
//! and File traits. 
//! The VFSDirectory and VFSFile are intended to be used as regular nodes within the filesystem
//! that require no special functionality as well as for inspiration for creating other concrete implementations
//!s of the Directory and File traits. 

// #[macro_use] extern crate log;
extern crate alloc;
extern crate spin;
extern crate fs_node;
extern crate memory;

use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;
use alloc::sync::{Arc, Weak};
use alloc::collections::BTreeMap;
use fs_node::{DirRef, WeakDirRef, Directory, FileOrDir, FsNode};


/// A struct that represents a node in the VFS 
pub struct VFSDirectory {
    /// The name of the directory
    pub name: String,
    /// A list of child filesystem nodes
    pub children: BTreeMap<String, FileOrDir>,
    /// A weak reference to the parent directory
    pub parent: WeakDirRef,
}

impl VFSDirectory {
    /// Creates a new directory and passes a pointer to the new directory created as output
    pub fn create(name: String, parent: &DirRef)  -> Result<DirRef, &'static str> {
        // creates a copy of the parent pointer so that we can add the newly created folder to the parent's children later
        let directory = VFSDirectory {
            name,
            children: BTreeMap::new(),
            parent: Arc::downgrade(parent),
        };
        let dir_ref = Arc::new(Mutex::new(directory)) as DirRef;
        parent.lock().insert(FileOrDir::Dir(dir_ref.clone()))?;
        Ok(dir_ref)
    }
}

impl Directory for VFSDirectory {
    fn insert(&mut self, node: FileOrDir) -> Result<Option<FileOrDir>, &'static str> {
        let name = node.get_name();
        if let Some(mut old_node) = self.children.insert(name, node) {
            old_node.set_parent_dir(Weak::<Mutex<VFSDirectory>>::new());
            Ok(Some(old_node))
        } else {
            Ok(None)
        }
    }

    fn get(&self, name: &str) -> Option<FileOrDir> {
        self.children.get(name).cloned()
    }

    /// Returns a string listing all the children in the directory
    fn list(&self) -> Vec<String> {
        self.children.keys().cloned().collect()
    }

    fn remove(&mut self, node: &FileOrDir) -> Option<FileOrDir> {
        if let Some(mut old_node) = self.children.remove(&node.get_name()) {
            old_node.set_parent_dir(Weak::<Mutex<VFSDirectory>>::new());
            Some(old_node)
        } else {
            None
        }
    }
}

impl FsNode for VFSDirectory {
    fn get_name(&self) -> String {
        self.name.clone()
    }

    /// Returns a pointer to the parent if it exists
    fn get_parent_dir(&self) -> Option<DirRef> {
        self.parent.upgrade()
    }

    fn set_parent_dir(&mut self, new_parent: WeakDirRef) {
        self.parent = new_parent;
    }
}
