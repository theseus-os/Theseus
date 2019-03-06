#![no_std]
#![feature(alloc)]

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
use alloc::boxed::Box;
use spin::Mutex;
use alloc::sync::{Arc, Weak};
use alloc::collections::BTreeMap;
use fs_node::{DirRef, FileRef, WeakDirRef, Directory, FileOrDir, File, FsNode};
use memory::MappedPages;


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
    pub fn new(name: String, parent: &DirRef)  -> Result<DirRef, &'static str> {
        // creates a copy of the parent pointer so that we can add the newly created folder to the parent's children later
        let directory = VFSDirectory {
            name: name,
            children: BTreeMap::new(),
            parent: Arc::downgrade(parent),
        };
        let dir_ref = Arc::new(Mutex::new(Box::new(directory) as Box<Directory + Send>));
        parent.lock().insert_child(FileOrDir::Dir(dir_ref.clone()))?;
        Ok(dir_ref)
    }
}

impl Directory for VFSDirectory {
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
    pub fn new(name: String, size: usize, contents: String, parent: &DirRef) -> Result<FileRef, &'static str> {
        // creates a copy of the parent pointer so that we can add the newly created folder to the parent's children later
        let file = VFSFile {
            name: name, 
            size: size, 
            contents: contents,
            parent: Arc::downgrade(parent),
        };
        let file_ref = Arc::new(Mutex::new(Box::new(file) as Box<File + Send>));
        parent.lock().insert_child(FileOrDir::File(file_ref.clone()))?;
        Ok(file_ref)
    }
}

impl File for VFSFile {
    fn read(&self, _buf: &mut [u8], offset: usize) -> Result<usize, &'static str> { unimplemented!()    }
    fn write(&mut self, _buf: &[u8], offset: usize) -> Result<usize, &'static str> { unimplemented!(); }
    
    fn delete(self) -> Result<(), &'static str> {
        Err("unimplemented")
    }
    
    fn size(&self) -> usize {
        self.size
    }
    
    fn as_mapping(&self) -> Result<&MappedPages, &'static str> {
        Err("cannot treat a VFSFile as a memory mapped region")
    }
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