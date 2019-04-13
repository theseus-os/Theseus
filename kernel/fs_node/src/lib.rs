#![no_std]
#![feature(alloc)]
//! Defines traits for Files and Directories within the virtual filesystem. These files and directories mimic 
//! that of a standard unix virtual filesystem, where directories follow a hierarchical system
//! and all directories have a parent directory (except for the special root directory). 
//! All files must be contained within directories. 
//! 
//! Note that both File and Directory extend from FsNode, which is a trait that defines
//! common methods for both Files and Directories to enhance code reuse 
//! 
//! Some functions return an enum FileOrDir; this allows us to seamlessly call functions on the return types of
//! other filesystem functions, and then we simply match on the FSnode to extract the concrete type
//! to perform the desired function

#[macro_use] extern crate alloc;
extern crate spin;
extern crate memory;

use alloc::string::String;
use alloc::vec::Vec;
use alloc::boxed::Box;
use spin::Mutex;
use alloc::sync::{Arc, Weak};
use memory::MappedPages;

/// An strong reference (Arc) and a Mutex wrapper around a Directory
/// This is a trait object that will allow us to seamlessly call fs methods on different 
/// concrete implementations of Directories 
pub type DirRef =  Arc<Mutex<Directory + Send>>;
/// An weak reference (Weak) and a Mutex wrapper around a Directory
pub type WeakDirRef = Weak<Mutex<Directory + Send>>;
/// A strong reference to a trait object that implements File. 
pub type FileRef = Arc<Mutex<File + Send>>;
/// A weak reference (Weak) and a Mutex wrapper around a File
pub type WeakFileRef = Weak<Mutex<File + Send>>;

/// Traits that both files and directories share
pub trait FsNode {
    /// Recursively gets the absolute pathname as a String
    fn get_absolute_path(&self) -> String {
        let mut path = self.get_name();
        if let Ok(cur_dir) =  self.get_parent_dir()  {
            let parent_path = &cur_dir.lock().get_absolute_path();
            // Check if the parent path is root 
            if parent_path == "/" {
                path.insert_str(0, &format!("{}", parent_path));
                return path;
            }
            path.insert_str(0, &format!("{}/", parent_path));
            return path;
        }
        return path;
    }
    /// Returns the string name of the node
    fn get_name(&self) -> String;
    /// Gets a pointer to the parent directory of the current node
    fn get_parent_dir(&self) -> Result<DirRef, &'static str>;
} 

// Trait for files, implementors of File must also implement FsNode
pub trait File : FsNode {
    /// Reads the contents of this file starting at the given `offset` and copies them into the given `buffer`.
    /// The length of the given `buffer` determines the maximum number of bytes to be read.
    fn read(&self, buffer: &mut [u8], offset: usize) -> Result<usize, &'static str>; 
    /// Writes the given `buffer` to this file starting at the given `offset`.
    fn write(&mut self, buffer: &[u8], offset: usize) -> Result<usize, &'static str>;
    /// Returns the size in bytes of this file.
    fn size(&self) -> usize;
    /// Returns a view of this file as an immutable memory-mapped region.
    fn as_mapping(&self) -> Result<&MappedPages, &'static str>;
}

/// Trait for directories, implementors of Directory must also implement FsNode
pub trait Directory : FsNode {
    /// Gets the file or directory from the current directory based on its name.
    fn get(&self, name: &str) -> Option<FileOrDir>; 
    /// Inserts the given new file or directory into this directory.
    /// If an existing node has the same name, that node is replaced and returned.
    fn insert(&mut self, node: FileOrDir) -> Result<Option<FileOrDir>, &'static str>;
    // Removes a file or directory from this directory.
    fn remove(&mut self, node: &FileOrDir) -> Result<(), &'static str>;
    /// Lists the names of the nodes in this directory.
    fn list(&mut self) -> Vec<String>;
}

/// Allows us to return a generic type that can be matched by the caller to extract the underlying type
#[derive(Clone)]
pub enum FileOrDir {
    File(FileRef),
    Dir(DirRef),
}

// Allows us to call methods directly on an enum so we don't have to match on the underlying type
impl FsNode for FileOrDir {
    /// Recursively gets the absolute pathname as a String
    fn get_absolute_path(&self) -> String {
        match self {
            FileOrDir::File(file) => file.lock().get_absolute_path(),
            FileOrDir::Dir(dir) => dir.lock().get_absolute_path(),
        }
    }
    fn get_name(&self) -> String {
        return match self {
            FileOrDir::File(file) => file.lock().get_name(),
            FileOrDir::Dir(dir) => dir.lock().get_name(),
        };
    }
    fn get_parent_dir(&self) -> Result<DirRef, &'static str> {
        return match self {
            FileOrDir::File(file) => file.lock().get_parent_dir(),
            FileOrDir::Dir(dir) => dir.lock().get_parent_dir(),
        };
    }
}
