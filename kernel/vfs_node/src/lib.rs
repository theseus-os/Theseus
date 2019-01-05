#![no_std]
#![feature(alloc)]

/// This crate contains a very basic, generic concrete implementation of the Directory
/// and File traits. 
/// The VFSDirectory and VFSFile are intended to be used as regular nodes within the filesystem
/// that require no special functionality as well as for inspiration for creating other concrete implementations
/// of the Directory and File traits. 

#[macro_use] extern crate log;
#[macro_use] extern crate alloc;
extern crate spin;
extern crate fs_node;

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use alloc::boxed::Box;
use spin::Mutex;
use alloc::sync::{Arc, Weak};
use alloc::collections::BTreeMap;
use fs_node::{StrongAnyDirRef, WeakDirRef, Directory, FSNode, File, FileDirectory};


/// A struct that represents a node in the VFS 
pub struct VFSDirectory {
    /// The name of the directory
    name: String,
    /// A list of StrongDirRefs or pointers to the child directories   
    children: BTreeMap<String, FSNode>,
    /// A weak reference to the parent directory, wrapped in Option because the root directory does not have a parent
    parent: WeakDirRef,
}

impl VFSDirectory {
    /// Creates a new directory and passes a reference to the new directory created as output
    pub fn new_dir(name: String, parent_pointer: WeakDirRef)  -> StrongAnyDirRef {
        let directory = VFSDirectory {
            name: name,
            children: BTreeMap::new(),
            parent: parent_pointer,
        };
        let dir_ref = Arc::new(Mutex::new(Box::new(directory) as Box<Directory + Send>));
        dir_ref
    }
}

impl Directory for VFSDirectory {
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
                None => Err("file/directory does not exist")
            }

    }

    /// Returns a string listing all the children in the directory
    fn list_children(&mut self) -> Vec<String> {
        return self.children.keys().cloned().collect();
    }
}

impl FileDirectory for VFSDirectory {
    fn get_name(&self) -> String {
        self.name.clone()
    }

    /// Returns a pointer to the parent if it exists
    fn get_parent_dir(&self) -> Result<StrongAnyDirRef, &'static str> {
        return match self.parent.upgrade() {
            Some(parent) => Ok(parent),
            None => Err("could not upgrade parent")
        }
    }

    fn set_parent(&mut self, parent_pointer: WeakDirRef) {
        self.parent = parent_pointer;
    }
}

pub struct VFSFile {
    /// The name of the file
    name: String,
    /// The file size 
    size: usize, 
    /// The string contents as a file: this primitive can be changed into a more complex struct as files become more complex
    contents: String,
    /// A weak reference to the parent directory
    parent: WeakDirRef,
}

impl VFSFile {
    pub fn new(name: String, size: usize, contents: String, parent: WeakDirRef) -> VFSFile {
        VFSFile {
            name: name, 
            size: size, 
            contents: contents,
            parent: parent
        }
    }
}

impl File for VFSFile {
    fn read(&self) -> String { 
        return self.contents.clone();
     }
    fn write(&mut self) { unimplemented!(); }
    fn seek(&self) { unimplemented!(); }
    fn delete(&self) { unimplemented!(); }
}

impl FileDirectory for VFSFile {
    fn get_name(&self) -> String {
        self.name.clone()
    }
    
    /// Returns a pointer to the parent if it exists
    fn get_parent_dir(&self) -> Result<StrongAnyDirRef, &'static str> {
        return match self.parent.upgrade() {
            Some(parent) => Ok(parent),
            None => Err("could not upgrade parent")
        }
    }

    fn set_parent(&mut self, parent_pointer: WeakDirRef) {
        self.parent = parent_pointer
    }
}