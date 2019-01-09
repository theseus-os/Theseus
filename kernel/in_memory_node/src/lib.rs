#![no_std]
#![feature(alloc)]

/// This crate contains a very basic, generic concrete implementation of the Directory
/// and File traits. 
/// The VFSDirectory and InMemoryFile are intended to be used as regular nodes within the filesystem
/// that require no special functionality as well as for inspiration for creating other concrete implementations
/// of the Directory and File traits. 

#[macro_use] extern crate log;
#[macro_use] extern crate alloc;
extern crate spin;
extern crate fs_node;
extern crate memory;

use core::mem;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use alloc::boxed::Box;
use spin::Mutex;
use alloc::sync::{Arc, Weak};
use alloc::collections::BTreeMap;
use fs_node::{StrongAnyDirRef, WeakDirRef, Directory, FSNode, File, FileDirectory};
use memory::MappedPages;


pub struct InMemoryFile {
    /// The name of the file
    name: String,
    /// The file size 
    size: usize, 
    /// The string contents as a file: this primitive can be changed into a more complex struct as files become more complex
    contents: MappedPages,
    /// A weak reference to the parent directory
    parent: WeakDirRef,
}

impl InMemoryFile {
    pub fn new(name: String, size: usize, contents: MappedPages, parent: WeakDirRef) -> InMemoryFile {
        InMemoryFile {
            name: name, 
            size: size, 
            contents: contents,
            parent: parent
        }
    }
}

impl File for InMemoryFile {
    type ContentType = MappedPages;
    fn read(&self) -> Self::ContentType { unimplemented!(); }
    fn write(&mut self, contents: Self::ContentType) -> Result<(), &'static str> { unimplemented!(); }
    fn seek(&self) { unimplemented!(); }
    fn delete(&self) { unimplemented!(); }
}

impl FileDirectory for InMemoryFile {
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