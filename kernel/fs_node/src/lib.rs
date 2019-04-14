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
use spin::Mutex;
use alloc::sync::{Arc, Weak};
use memory::MappedPages;


/// A reference to any type that implements the Directory trait.
pub type DirRef =  Arc<Mutex<Directory + Send>>;
/// A weak reference to any type that implements the Directory trait.
pub type WeakDirRef = Weak<Mutex<Directory + Send>>;
/// A reference to any type that implements the File trait.
pub type FileRef = Arc<Mutex<File + Send>>;
/// A weak reference to any type that implements the File trait.
pub type WeakFileRef = Weak<Mutex<File + Send>>;


/// A trait that covers any filesystem node, both files and directories.
pub trait FsNode {
    /// Recursively gets the absolute pathname as a String
    fn get_absolute_path(&self) -> String {
        let mut path = self.get_name();
        if let Some(cur_dir) =  self.get_parent_dir()  {
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

    /// Returns the parent directory of the current node.
    fn get_parent_dir(&self) -> Option<DirRef>;

    /// Sets this node's parent directory.
    /// This is useful for ensuring correctness when inserting or remonving 
    /// files or directories from their parent directory.
    fn set_parent_dir(&mut self, new_parent: WeakDirRef);
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
    /// 
    /// Note that this function **does not** set the given `node`'s parent directory;
    /// that should be set when the `node` was originally created, before calling this function. 
    /// However, if a node is replaced, that old node's parent directory will be cleared
    /// to reflect that it is no longer in this directory.
    /// 
    /// The lock on `node` must not be held because it will be acquired within this function.
    fn insert(&mut self, node: FileOrDir) -> Result<Option<FileOrDir>, &'static str>;

    /// Removes a file or directory from this directory and returns it if found.
    /// Also, the returned node's parent directory reference is cleared.
    /// 
    /// The lock on `node` must not be held because it will be acquired within this function.
    fn remove(&mut self, node: &FileOrDir) -> Option<FileOrDir>;

    /// Lists the names of the nodes in this directory.
    fn list(&self) -> Vec<String>;
}

/// Allows us to return a generic type that can be matched by the caller to extract the underlying type
#[derive(Clone)]
pub enum FileOrDir {
    File(FileRef),
    Dir(DirRef),
}

// Allows us to call methods directly on an enum so we don't have to match on the underlying type
impl FsNode for FileOrDir {
    
    fn get_absolute_path(&self) -> String {
        match self {
            FileOrDir::File(file) => file.lock().get_absolute_path(),
            FileOrDir::Dir(dir) => dir.lock().get_absolute_path(),
        }
    }

    fn get_name(&self) -> String {
        match self {
            FileOrDir::File(file) => file.lock().get_name(),
            FileOrDir::Dir(dir) => dir.lock().get_name(),
        }
    }

    fn get_parent_dir(&self) -> Option<DirRef> {
        match self {
            FileOrDir::File(file) => file.lock().get_parent_dir(),
            FileOrDir::Dir(dir) => dir.lock().get_parent_dir(),
        }
    }

    fn set_parent_dir(&mut self, new_parent: WeakDirRef) {
        match self {
            FileOrDir::File(file) => file.lock().set_parent_dir(new_parent),
            FileOrDir::Dir(dir) => dir.lock().set_parent_dir(new_parent),
        }
    }
}
