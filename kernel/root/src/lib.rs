#![no_std]
#![feature(alloc)]
/// This crate contains the implementation of the special root directory. The only way that this 
/// directory implementation differs from VFSDirectory is that there is no parent field (becuase the 
/// root has no parent directory), and that internal calls to parent will return some type of error value

#[macro_use] extern crate log;
#[macro_use] extern crate alloc;
#[macro_use] extern crate lazy_static;
extern crate spin;
extern crate fs_node;

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use alloc::boxed::Box;
use spin::Mutex;
use alloc::sync::{Arc, Weak};
use alloc::collections::BTreeMap;
use fs_node::{StrongAnyDirRef, WeakDirRef, Directory, FSNode, FileDirectory};

lazy_static! {
    /// The root directory
    /// Returns a tuple for easy access to the name of the root so we don't have to lock it
    pub static ref ROOT: (String, StrongAnyDirRef) = {
        let root_dir = RootDirectory {
            name: "/root".to_string(),
            children: BTreeMap::new() 
        };
        (String::from("/root"), Arc::new(Mutex::new(Box::new(root_dir))))
    };
}

pub fn get_root() -> StrongAnyDirRef {
    Arc::clone(&ROOT.1)
}

/// A struct that represents a node in the VFS 
pub struct RootDirectory {
    /// The name of the directory
    name: String,
    /// A list of StrongDirRefs or pointers to the child directories   
    children: BTreeMap<String, FSNode>,
}

impl Directory for RootDirectory {
    fn add_fs_node(&mut self, new_fs_node: FSNode) -> Result<(), &'static str> {
        match new_fs_node {
            FSNode::Dir(dir) => {
                let name = dir.lock().get_name().clone();
                self.children.insert(name, FSNode::Dir(dir));
                },
            FSNode::File(file) => {
                let name = file.lock().get_name().clone();
                self.children.insert(name, FSNode::File(file));
                },
        }
        Ok(())
    }

    fn get_child(&mut self, child_name: String, is_file: bool) -> Result<FSNode, &'static str> {
        let option_child = self.children.get(&child_name);
        match option_child {
            Some(child) => match child {
                FSNode::File(file) => {
                        return Ok(FSNode::File(Arc::clone(file)));
                    }
                FSNode::Dir(dir) => {
                        return Ok(FSNode::Dir(Arc::clone(dir)));
                    }
            },
            None => Err("could not get child from children map")
        }
    }

    /// Returns a string listing all the children in the directory
    fn list_children(&mut self) -> Vec<String> {
        return self.children.keys().cloned().collect();
    }
}

impl FileDirectory for RootDirectory {
    /// Functions as pwd command in bash, recursively gets the absolute pathname as a String
    fn get_path_as_string(&self) -> String {
        return "/root".to_string();
    }

    fn get_name(&self) -> String {
        self.name.clone()
    }

    /// Returns a pointer to the parent if it exists
    fn get_parent_dir(&self) -> Result<StrongAnyDirRef, &'static str> {
        return Err("root does not have a parent");
    }

    fn get_self_pointer(&self) -> Result<StrongAnyDirRef, &'static str> {
        return Ok(get_root());
    }

    fn set_parent(&mut self, parent_pointer: WeakDirRef) {
        // root doesn't have a parent
        return;
    }
}
