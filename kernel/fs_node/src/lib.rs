#![no_std]
#![feature(alloc)]
/// Defines traits for Files and Directories within the virtual filesystem. These files and directories mimic 
/// that of a standard unix virtual filesystem, where directories follow a hierarchical system
/// and all directories have a parent directory (except for the special root directory). 
/// All files must be contained within directories. 
/// 
/// Note that both File and Directory extend from FileDirectory, which is a trait that defines
/// common methods for both Files and Directories to enhance code reuse 
/// 
/// Some functions return an enum FSNode; this allows us to seamlessly call functions on the return types of
/// other filesystem functions, and then we simply match on the FSnode to extract the concrete type
/// to perform the desired function

#[macro_use] extern crate alloc;
extern crate spin;

use alloc::string::String;
use alloc::vec::Vec;
use alloc::boxed::Box;
use spin::Mutex;
use alloc::sync::{Arc, Weak};
use core::any::Any;

/// An strong reference (Arc) and a Mutex wrapper around the generic Directory
/// This is a trait object that will allow us to seamlessly call fs methods on different 
/// concrete implementations of Directories 
pub type StrongDirRef = Arc<Mutex<Box<Directory + Send>>>;

/// An weak reference (Weak) and a Mutex wrapper around VFSDirectory
pub type WeakDirRef = Weak<Mutex<Box<Directory + Send>>>;
pub type StrongFileRef = Arc<Mutex<Box<File + Send>>>;

/// Traits that both files and directories share
pub trait FileDirectory {
    /// Recursively gets the absolute pathname as a String
    fn get_path_as_string(&self) -> String {
        let mut path = self.get_name();
        if let Ok(cur_dir) =  self.get_parent_dir() {
            path.insert_str(0, &format!("{}/",&cur_dir.lock().get_path_as_string()));
            return path;
        }
        return path;
    }
    /// Returns the name of the File or Directory
    fn get_name(&self) -> String;
    /// Returns the parent of the File or Directory in the form of an Arc Mutex
    fn get_parent_dir(&self) -> Result<StrongDirRef, &'static str>;
    /// Returns a strong reference to itself
    fn get_self_pointer(&self) -> Result<StrongDirRef, &'static str> {
        let parent = match self.get_parent_dir() {
            Ok(parent) => parent,
            Err(err) => return Err(err)
        };

        let mut locked_parent = parent.lock();
        match locked_parent.get_child(self.get_name(), false) {
            Ok(child) => {
                match child {
                    FSNode::Dir(dir) => Ok(dir),
                    FSNode::File(_file) => Err("should not be a file"),
                }
            },
            Err(err) => return Err(err)
        }
    }
}

// Traits for files, implementors of File must also implement FileDirectory
pub trait File : FileDirectory {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, &'static str>; 
    fn write(&mut self, buf: &[u8]) -> Result<usize, &'static str>;
    fn seek(&self); 
    fn delete(&self);
    fn size(&self) -> usize;
}

/// Traits for directories, implementors of Directory must also implement FileDirectory
pub trait Directory : FileDirectory + Send {
    fn get_child(&mut self, child_name: String, is_file: bool) -> Result<FSNode, &'static str>; 
    fn insert_child(&mut self, child: FSNode) -> Result<(), &'static str>;
    fn list_children(&mut self) -> Vec<String>;
}

/// Allows us to return a generic type that can be matched by the caller to extract the underlying type
#[derive(Clone)]
pub enum FSNode {
    File(StrongFileRef),
    Dir(StrongDirRef),
}

// Allows us to call methods directly on an enum so we don't have to match on the underlying type
impl FileDirectory for FSNode {
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
    fn get_parent_dir(&self) -> Result<StrongDirRef, &'static str> {
        return match self {
            FSNode::File(file) => file.lock().get_parent_dir(),
            FSNode::Dir(dir) => dir.lock().get_parent_dir(),
        };
    }
    fn get_self_pointer(&self) -> Result<StrongDirRef, &'static str> {
        return match self {
            FSNode::File(file) => file.lock().get_self_pointer(),
            FSNode::Dir(dir) => dir.lock().get_self_pointer(),
        };
    }
}