#![no_std]
#![feature(alloc)]

//! This crate contains a very basic, generic concrete implementation of the Directory
//! and File traits. 
//! The VFSDirectory and VFSFile are intended to be used as regular nodes within the filesystem
//! that require no special functionality as well as for inspiration for creating other concrete implementations
//!s of the Directory and File traits. 

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
use fs_node::{DirRef, FileRef, WeakDirRef, Directory, FileOrDir, File, FsNode};


/// A struct that represents a node in the VFS 
pub struct VFSDirectory {
    /// The name of the directory
    pub name: String,
    /// A list of DirRefs or pointers to the child directories   
    pub children: BTreeMap<String, FileOrDir>,
    /// A weak reference to the parent directory, wrapped in Option because the root directory does not have a parent
    pub parent: WeakDirRef,
}

impl VFSDirectory {
    /// Creates a new directory and passes a pointer to the new directory created as output
    pub fn new_dir(name: String, parent_pointer: WeakDirRef)  -> Result<DirRef, &'static str> {
        // creates a copy of the parent pointer so that we can add the newly created folder to the parent's children later
        let parent_copy = Weak::clone(&parent_pointer);
        let directory = VFSDirectory {
            name: name,
            children: BTreeMap::new(),
            parent: parent_pointer,
        };
        let dir_ref = Arc::new(Mutex::new(Box::new(directory) as Box<Directory + Send>));
        // create a copy of the newly created directory so we can return it
        let dir_ref_copy = Arc::clone(&dir_ref);
        let strong_parent = Weak::upgrade(&parent_copy).ok_or("could not upgrade parent")?;
        strong_parent.lock().insert_child(FileOrDir::Dir(dir_ref))?;
        return Ok(dir_ref_copy)
    }
}

impl Directory for VFSDirectory {
    fn insert_child(&mut self, child: FileOrDir) -> Result<(), &'static str> {
        // gets the name of the child node to be added
        let name = child.get_name();
        self.children.insert(name, child);
        return Ok(())
    }

    fn get_child(&mut self, child_name: String, is_file: bool) -> Result<FileOrDir, &'static str> {
        let option_child = self.children.get(&child_name);
            match option_child {
                Some(child) => match child {
                    FileOrDir::File(file) => {
                            return Ok(FileOrDir::File(Arc::clone(file)));
                        }
                    FileOrDir::Dir(dir) => {
                            return Ok(FileOrDir::Dir(Arc::clone(dir)));
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

impl FsNode for VFSDirectory {
    fn get_name(&self) -> String {
        self.name.clone()
    }

    /// Returns a pointer to the parent if it exists
    fn get_parent_dir(&self) -> Result<DirRef, &'static str> {
        self.parent.upgrade().ok_or("couldn't upgrade parent")
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
    pub fn new(name: String, size: usize, contents: String, parent: WeakDirRef) -> Result<FileRef, &'static str>{
        // creates a copy of the parent pointer so that we can add the newly created folder to the parent's children later
        let parent_copy = Weak::clone(&parent);
        let file = VFSFile {
            name: name, 
            size: size, 
            contents: contents,
            parent: parent
        };
        let file_pointer = Arc::new(Mutex::new(Box::new(file) as Box<File + Send>));
        // create a copy of the newly created directory so we can return it
        let file_pointer_copy = Arc::clone(&file_pointer);
        let strong_parent = Weak::upgrade(&parent_copy).ok_or("could not upgrade parent")?;
        strong_parent.lock().insert_child(FileOrDir::File(file_pointer))?;
        return Ok(file_pointer_copy)
    }
}

impl File for VFSFile {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, &'static str> { unimplemented!()    }
    fn write(&mut self, buf: &[u8]) -> Result<usize, &'static str> { unimplemented!(); }
    fn seek(&self) { unimplemented!(); }
    fn delete(self) { unimplemented!(); }
    fn size(&self) -> usize {unimplemented!()}
}

impl FsNode for VFSFile {
    fn get_name(&self) -> String {
        self.name.clone()
    }
    
    /// Returns a pointer to the parent if it exists
    fn get_parent_dir(&self) -> Result<DirRef, &'static str> {
        self.parent.upgrade().ok_or("couldn't upgrade parent")
    }
}