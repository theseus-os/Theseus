#![no_std]
#![feature(alloc)]
//! Defines traits for Files and Directories within the virtual filesystem. These files and directories mimic 
//! that of a standard unix virtual filesystem, where directories follow a hierarchical system
//! and all directories have a parent directory (except for the special root directory). 
//! All files must be contained within directories. 
//! 
//! Note that both File and Directory extend from FSCompatible, which is a trait that defines
//! common methods for both Files and Directories to enhance code reuse 
//! 
//! Some functions return an enum FSNode; this allows us to seamlessly call functions on the return types of
//! other filesystem functions, and then we simply match on the FSnode to extract the concrete type
//! to perform the desired function

#[macro_use] extern crate alloc;
extern crate spin;

use alloc::string::String;
use alloc::vec::Vec;
use alloc::boxed::Box;
use spin::Mutex;
use alloc::sync::{Arc, Weak};
use core::any::Any;

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
pub trait FSCompatible {
    /// Recursively gets the absolute pathname as a String
    fn get_path_as_string(&self) -> String {
        let mut path = self.get_name();
        if let Ok(cur_dir) =  self.get_parent_dir() {
            path.insert_str(0, &format!("{}/",&cur_dir.lock().get_path_as_string()));
            return path;
        }
        return path;
    }
    /// Returns the string name of the node
    fn get_name(&self) -> String;
    /// Gets a pointer to the parent directory of the current node
    fn get_parent_dir(&self) -> Result<DirRef, &'static str>;
} 

// Traits for files, implementors of File must also implement FSCompatible
pub trait File : FSCompatible {
    /// Reads the bytes from a file: implementors should pass in an empty buffer that the read function writes the file's data to
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, &'static str>; 
    /// Writes the bytes argument to the contents of the file
    fn write(&mut self, buf: &[u8]) -> Result<usize, &'static str>;
    /// Finds a string sequence within the file
    fn seek(&self); 
    /// Deletes the file
    fn delete(self);
    /// Returns the size of the actual file content (i.e. the bytes that correspond to user-meaningful information) 
    fn size(&self) -> usize;
}

/// Traits for directories, implementors of Directory must also implement FSCompatible
pub trait Directory : FSCompatible + Send {
    /// Gets an individual child node from the current directory based on the name field of that node
    fn get_child(&mut self, child_name: String, is_file: bool) -> Result<FSNode, &'static str>; 
    /// Inserts a child into whatever collection the Directory uses to track children nodes
    fn insert_child(&mut self, child: FSNode) -> Result<(), &'static str>;
    /// Lists the names of the children nodes of the current directory
    fn list_children(&mut self) -> Vec<String>;
}

/// Allows us to return a generic type that can be matched by the caller to extract the underlying type
#[derive(Clone)]
pub enum FSNode {
    File(FileRef),
    Dir(DirRef),
}

// Allows us to call methods directly on an enum so we don't have to match on the underlying type
impl FSCompatible for FSNode {
    /// Recursively gets the absolute pathname as a String
    fn get_path_as_string(&self) -> String {
        return match self {
            FSNode::File(file) => file.lock().get_path_as_string(),
            FSNode::Dir(dir) => dir.lock().get_path_as_string(),
        };
    }
    fn get_name(&self) -> String {
        return match self {
            FSNode::File(file) => file.lock().get_name(),
            FSNode::Dir(dir) => dir.lock().get_name(),
        };
    }
    fn get_parent_dir(&self) -> Result<DirRef, &'static str> {
        return match self {
            FSNode::File(file) => file.lock().get_parent_dir(),
            FSNode::Dir(dir) => dir.lock().get_parent_dir(),
        };
    }
}