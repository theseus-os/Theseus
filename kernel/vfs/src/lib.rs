#![no_std]
#![feature(alloc)]

#[macro_use] extern crate log;
#[macro_use] extern crate alloc;
#[macro_use] extern crate lazy_static;
extern crate spin;

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use spin::Mutex;
use alloc::arc::{Arc, Weak};


lazy_static! {
    /// The root directory
    pub static ref ROOT: StrongDirRef = {
        let root_dir = Directory {
            basename: "/root".to_string(),
            child_dirs: Vec::new(),
            files: Vec::new(),
            parent: Weak::new(), 
        };
        Arc::new(Mutex::new(root_dir))
    };
}

pub fn get_root() -> StrongDirRef {
    Arc::clone(&ROOT)
}

/// An strong reference (Arc) and a Mutex wrapper around Directory
pub type StrongDirRef = Arc<Mutex<Directory>>;
/// An weak reference (Weak) and a Mutex wrapper around Directory
pub type WeakDirRef = Weak<Mutex<Directory>>;

pub struct Directory{
    /// The name of the directory, the last part of pathname
    basename: String,
    /// A list of StrongDirRefs or pointers to the child directories 
    child_dirs: Vec<StrongDirRef>,
    /// A list of files within this directory
    files: Vec<File>,
    /// A weak reference to the parent directory 
    parent: WeakDirRef,
}


impl Directory {
    /// Assumes you actually want to open the file upon creation
    pub fn new_file(&mut self, basename: String, parent_pointer: WeakDirRef)  {
        let file = File {
            basename: basename,
            size: 0,
            parent: parent_pointer,
        };
        self.files.push(file);
    }

    /// Creates a new directory and passes a reference to the new directory created as output
    pub fn new_dir(&mut self, basename: String, parent_pointer: WeakDirRef) -> StrongDirRef {
        let directory = Directory {
            basename: basename,
            child_dirs: Vec::new(),
            files:  Vec::new(),
            parent: parent_pointer,
        };
        let dir_ref = Arc::new(Mutex::new(directory));
        self.child_dirs.push(dir_ref.clone());
        dir_ref
    }
    
    /// Looks for the child directory specified by dirname and returns a reference to it 
    pub fn get_child_dir(&self, child_dir: String) -> Option<StrongDirRef> {
        for dir in self.child_dirs.iter() {
            if dir.lock().basename == child_dir {
                return Some(Arc::clone(dir));
            }
        }
        return None;
    }

    pub fn get_parent_dir(&self) -> Option<StrongDirRef> {
        self.parent.upgrade()
    }

    /// Returns a string listing all the children in the directory
    pub fn list_children(&mut self) -> String {
        let mut children_list = String::new();
        for dir in self.child_dirs.iter() {
            children_list.push_str(&format!("{}, ",dir.lock().basename));
        }

        for file in self.files.iter() {
            children_list.push_str(&format!("{}, ", file.basename));
        }
        return children_list;
    }
    
    /// Functions as pwd command in bash, recursively gets the absolute pathname
    pub fn get_path(&self) -> String {
        let mut path = self.basename.clone();
        if let Some(cur_dir) =  self.parent.upgrade() {
            path.insert_str(0, &format!("{}/",&cur_dir.lock().get_path()));
            return path;
        }
        return path
    }
}

pub struct File {
    /// The name of the file
    basename: String,
    /// The file size 
    size: usize, 
    /// A weak reference to the parent directory
    parent: WeakDirRef,
}

// Basic traits fo file and directory 
trait FileOperations {
    fn create(name: String) -> Self;
    fn write(&mut self);
    fn read(&self);
    fn delete(&self);
}