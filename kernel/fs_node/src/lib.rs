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

/// An strong reference (Arc) to a trait object that implements Directory
/// This is a trait object that will allow us to seamlessly call fs methods on different 
/// concrete implementations of Directories 
pub type DirRef = Arc<Mutex<Box<Directory + Send>>>;
/// An weak reference (Weak) and a Mutex wrapper around a trait object that implements Directory
pub type WeakDirRef = Weak<Mutex<Box<Directory + Send>>>;
/// A strong reference to a trait object that implements file. We don't need a weak reference because there
/// should not be cyclic pointers from a file to another object
pub type FileRef = Arc<Mutex<Box<File + Send>>>;

/// Traits that both files and directories share
pub trait FsNode {
    /// Recursively gets the absolute pathname as a String
    fn get_path_as_string(&self) -> String {
        let mut path = self.get_name();
        if let Ok(cur_dir) =  self.get_parent_dir()  {
            let parent_path = &cur_dir.lock().get_path_as_string();
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

// Traits for files, implementors of File must also implement FsNode
pub trait File : FsNode {
    /// Reads the contents of this file into the given `buffer`
    /// Caller should pass in an empty buffer and the read function will query the size of the buffer
    fn read(&self, buffer: &mut [u8]) -> Result<usize, &'static str>; 
    /// Writes the bytes argument to the contents of the file
    fn write(&mut self, buffer: &[u8]) -> Result<usize, &'static str>;
    /// Deletes the file
    fn delete(self) -> Result<(), &'static str>;
    /// Returns the size of the actual file content (i.e. the bytes that correspond to user-meaningful information) 
    fn size(&self) -> usize;
    /// Returns a view of the file as an immutable memory-mapped region.
    fn as_mapping(&self) -> Result<&MappedPages, &'static str>;
}

/// Traits for directories, implementors of Directory must also implement FsNode
pub trait Directory : FsNode + Send {
    /// Gets an individual child node from the current directory based on the name field of that node
    fn get_child(&self, child_name: &str) -> Option<FileOrDir>; 
    /// Inserts a child into whatever collection the Directory uses to track children nodes
    fn insert_child(&mut self, child: FileOrDir) -> Result<Option<FileOrDir>, &'static str>;
    /// Lists the names of the children nodes of the current directory
    fn list_children(&mut self) -> Vec<String>;
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
    fn get_path_as_string(&self) -> String {
        match self {
            FileOrDir::File(file) => file.lock().get_path_as_string(),
            FileOrDir::Dir(dir) => dir.lock().get_path_as_string(),
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