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
            path: "/root".to_string(),
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
    /// The absolute path of the file from the root
    path: String,
    /// A list of StrongDirRefs or pointers to the child directories 
    child_dirs: Vec<StrongDirRef>,
    /// A list of files within this directory
    files: Vec<File>,
    /// A weak reference to the parent directory 
    parent: WeakDirRef,
}


impl Directory {
    /// Assumes you actually want to open the file upon creation
    pub fn new_file(&mut self, name: String, parent_pointer: WeakDirRef)  {
        let new_path = format!("{}/{}", self.get_path(), name);
        let file = File {
            path: new_path,
            size: 0,
            parent: parent_pointer,
        };
        self.files.push(file);
    }

    /// Creates a new directory and passes a reference to the new directory created as output
    pub fn new_dir(&mut self, name: String, parent_pointer: WeakDirRef) -> StrongDirRef {
        let directory = Directory {
            path: format!("{}/{}", self.path, name),
            child_dirs: Vec::new(),
            files:  Vec::new(),
            parent: parent_pointer,
        };
        let dir_ref = Arc::new(Mutex::new(directory));
        self.child_dirs.push(dir_ref.clone());
        dir_ref
    }
    
    /// Looks for the child directory specified by dirname and returns a reference to it 
    pub fn get_child_dir(&self, chdirname: String) -> Option<StrongDirRef> {
        for dir in self.child_dirs.iter() {
            if dir.lock().get_basename() == chdirname {
                return Some(Arc::clone(dir));
            }
        }
        return None;
    }
    
    /// Returns a string listing all the children in the directory
    pub fn list_children(&mut self) -> String {
        let mut children_list = String::new();
        for dir in self.child_dirs.iter() {
            children_list.push_str(&format!("{}, ",dir.lock().get_basename()));
        }

        for file in self.files.iter() {
            children_list.push_str(&format!("{}, ", file.get_basename()));
        }
        return children_list;
    }
    
    /// Functions as pwd command in bash, returns the full path as a string
    pub fn get_path(&self) -> String {
        return self.path.clone();
    }
    
    /// Gets the basename, last part of path
    pub fn get_basename(&self) -> String {
        let path: Vec<&str> = self.path.split("/").collect();
        return path[path.len() - 1].to_string();
    }
}

pub struct File {
    path: String,
    size: usize, 
    parent: WeakDirRef,
}

impl File {
    pub fn read(self) -> String {
        return format!("filepath: {}, size: {}, filetype: {}", self.path, self.size, String::from("temp filetype"));
    }
    
    /// Gets the basename, last part of path
    pub fn get_basename(&self) -> String {
        let path: Vec<&str> = self.path.split("/").collect();
        return path[path.len() - 1].to_string();
    }
}