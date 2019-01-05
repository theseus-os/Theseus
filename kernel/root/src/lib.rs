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
            children: BTreeMap::new(), 
            parent: None, 
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
    /// A weak reference to the parent directory, wrapped in Option because the root directory does not have a parent
    parent: Option<WeakDirRef>,
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
        let mut path = self.name.clone();
        if let Some(cur_dir) =  self.get_parent_dir() {
            path.insert_str(0, &format!("{}/",&cur_dir.lock().get_path_as_string()));
            return path;
        }
        return path;
    }

    fn get_name(&self) -> String {
        self.name.clone()
    }

    /// Returns a pointer to the parent if it exists
    fn get_parent_dir(&self) -> Option<StrongAnyDirRef> {
        match self.parent {
            Some(ref dir) => dir.upgrade(),
            None => None
        }
    }

    fn get_self_pointer(&self) -> Result<StrongAnyDirRef, &'static str> {
        if self.name == ROOT.0 {
            debug!("MATCHED TO ROOT");
            return Ok(get_root());
        }
        let weak_parent = match self.parent.clone() {
            Some(parent) => parent, 
            None => return Err("parent does not exist")
        };
        let parent = match Weak::upgrade(&weak_parent) {
            Some(weak_ref) => weak_ref,
            None => return Err("could not upgrade parent")
        };

        let mut locked_parent = parent.lock();
        match locked_parent.get_child(self.name.clone(), false) {
            Ok(child) => {
                match child {
                    FSNode::Dir(dir) => Ok(dir),
                    FSNode::File(_file) => Err("should not be a file"),
                }
            },
            Err(err) => {
                error!("failed in get_self_pointer because: {}", err);
                return Err(err);
                },
        }
    }

    fn set_parent(&mut self, parent_pointer: WeakDirRef) {
        self.parent = Some(parent_pointer);
    }
}
