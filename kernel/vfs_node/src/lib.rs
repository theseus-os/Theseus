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
use fs_node::{DirRef, FileRef, WeakDirRef, Directory, FileOrDir, File, FsNode};
use memory::MappedPages;


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
    pub fn new(name: String, parent: &DirRef)  -> Result<DirRef, &'static str> {
        // creates a copy of the parent pointer so that we can add the newly created folder to the parent's children later
        let directory = VFSDirectory {
            name: name,
            children: BTreeMap::new(),
            parent: Arc::downgrade(parent),
        };
        let dir_ref = Arc::new(Mutex::new(directory)) as Arc<Mutex<Directory + Send>>;
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

pub struct VFSFile {
    /// The name of the file
    name: String,
    /// The file size 
    size: usize, 
    /// The string contents as a file: this primitive can be changed into a more complex struct as files become more complex
    _contents: String,
    /// A weak reference to the parent directory
    parent: WeakDirRef,
}

impl VFSFile {
    pub fn new(name: String, size: usize, contents: String, parent: &DirRef) -> Result<FileRef, &'static str> {
        let file = VFSFile {
            name: name, 
            size: size, 
            _contents: contents,
            parent: Arc::downgrade(parent),
        };
        let file_ref = Arc::new(Mutex::new(file)) as Arc<Mutex<File + Send>>;
        parent.lock().insert(FileOrDir::File(file_ref.clone()))?;
        Ok(file_ref)
    }
}

impl File for VFSFile {
    fn read(&self, _buf: &mut [u8], _offset: usize) -> Result<usize, &'static str> { 
        Err("VFSFile::read() is unimplemented")
    }

    fn write(&mut self, _buf: &[u8], _offset: usize) -> Result<usize, &'static str> {
        Err("VFSFile::write() is unimplemented")
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
    
    fn get_parent_dir(&self) -> Option<DirRef> {
        self.parent.upgrade()
    }

    fn set_parent_dir(&mut self, new_parent: WeakDirRef) {
        self.parent = new_parent;
    }
}