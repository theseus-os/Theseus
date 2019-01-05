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

extern crate alloc;
extern crate spin;

use alloc::string::String;
use alloc::vec::Vec;
use alloc::boxed::Box;
use spin::Mutex;
use alloc::sync::{Arc, Weak};

/// An strong reference (Arc) and a Mutex wrapper around the generic Directory
/// This is a trait object that will allow us to seamlessly call fs methods on different 
/// concrete implementations of Directories 
pub type StrongDirRef<D: Directory + Send> = Arc<Mutex<D>>;
pub type StrongAnyDirRef = StrongDirRef<Box<Directory + Send>>;

/// An weak reference (Weak) and a Mutex wrapper around VFSDirectory
pub type WeakDirRef = Weak<Mutex<Box<Directory + Send>>>;
pub type StrongFileRef = Arc<Mutex<Box<File + Send>>>;

/// Traits that both files and directories share
pub trait FileDirectory {
    fn get_path_as_string(&self) -> String;
    fn get_name(&self) -> String;
    fn get_parent_dir(&self) -> Option<StrongAnyDirRef>;
    fn get_self_pointer(&self) -> Result<StrongAnyDirRef, &'static str>; // DON'T CALL THIS (add_fs_node performs this function)
    fn set_parent(&mut self, parent_pointer: WeakDirRef); // DON'T CALL THIS (add_fs_node performs this function)
}

// Traits for files, implementors of File must also implement FileDirectory
pub trait File : FileDirectory {
    fn read(&self) -> String;
    fn write(&mut self);
    fn seek(&self); 
    fn delete(&self);
}

/// Traits for directories, implementors of Directory must also implement FileDirectory
pub trait Directory : FileDirectory + Send {
    fn add_fs_node(&mut self, new_node: FSNode) -> Result<(), &'static str>;
    fn get_child(&mut self, child_name: String, is_file: bool) -> Result<FSNode, &'static str>; 
    fn list_children(&mut self) -> Vec<String>;
}

/// Allows us to return a generic type that can be matched by the caller to extract the underlying type
pub enum FSNode{
    File(StrongFileRef),
    Dir(StrongAnyDirRef),
}