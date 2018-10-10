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
            name: "/root".to_string(),
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
    /// The name of the directory
    name: String,
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
        let file = File {
            name: name,
            size: 0,
            parent: parent_pointer,
        };
        self.files.push(file);
    }

    /// Creates a new directory and passes a reference to the new directory created as output
    pub fn new_dir(&mut self, name: String, parent_pointer: WeakDirRef) -> StrongDirRef {
        let directory = Directory {
            name: name,
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
            if dir.lock().name == child_dir {
                return Some(Arc::clone(dir));
            }
        }
        return None;
    }

    /// Returns a pointer to the parent if it exists
    pub fn get_parent_dir(&self) -> Option<StrongDirRef> {
        self.parent.upgrade()
    }

    /// Returns a string listing all the children in the directory
    pub fn list_children(&mut self) -> String {
        let mut children_list = String::new();
        for dir in self.child_dirs.iter() {
            children_list.push_str(&format!("{}, ",dir.lock().name));
        }

        for file in self.files.iter() {
            children_list.push_str(&format!("{}, ", file.name));
        }
        return children_list;
    }

    /// Functions as pwd command in bash, recursively gets the absolute pathname as a String
    pub fn get_path_as_string(&self) -> String {
        let mut path = self.name.clone();
        if let Some(cur_dir) =  self.parent.upgrade() {
            path.insert_str(0, &format!("{}/",&cur_dir.lock().get_path_as_string()));
            return path;
        }
        return path;
    }

    pub fn get_path(&self) -> Path {
        Path::new(self.get_path_as_string())
    }
}

pub struct File {
    /// The name of the file
    name: String,
    /// The file size 
    size: usize, 
    /// A weak reference to the parent directory
    parent: WeakDirRef,
}

// Basic traits for File and Directory 
trait FileOperations {
    fn create(name: String) -> Self;
    fn write(&mut self);
    fn read(&self);
    fn delete(&self); 
}

/// A structure that represents a file path
pub struct Path {
    path: String, 
}

impl Path {
    /// Creates a new Path struct given its name
    pub fn new(path: String) -> Self {
        Path {
            path: path
        }
    }
    
    /// Returns the components of the path 
    fn components(&self) -> Vec<String> {
        let components = self.path.split("/").map(|s| s.to_string()).collect();
        return components;
    } 

    // /// Returns a canonical and absolute form of the current path
    // fn canonicalize(&self, wd: &StrongDirRef) -> Path {
    //     let mut new_components = Vec::new();
    //     // Push the components of the working directory to the components of the new path
    //     let current_path = wd.lock().get_path();
    //     new_components.extend(current_path.components());
    //     // Push components of the path to the components of the new path
    //     for component in self.components().iter() {
    //         if component == &String::from(".") {
    //             continue;
    //         } else if component == &String::from("..") {
    //             new_components.pop();
    //         } else {
    //             new_components.push(component.to_string());
    //         }
    //     }
    //     // Create the new path from its components 
    //     let mut new_path = String::new();
    //     for component in new_components.iter() {
    //         new_path.push_str(&format!("{}/",  component));
    //     }
    //     return Path::new(new_path);
    // }
    
    // /// Expresses the current Path, self, relative to another Path, other
    // /// https://docs.rs/pathdiff/0.1.0/src/pathdiff/lib.rs.html#32-74
    // pub fn relative(&self, other: Path) -> Option<Path> {
    //     let ita = self.components();
    //     let itb = other.components();
    //     let mut ita_iter = ita.iter();
    //     let mut itb_iter = itb.iter();
    //     let mut comps: Vec<String> = Vec::new();
    //     loop {
    //         match (ita_iter.next(), itb_iter.next()) {
    //             (None, None) => break,
    //             (Some(a), None) => {
    //                 comps.push(a.to_string());
    //                 break;
    //             }
    //             (None, _) => comps.push("..".to_string()),
    //             (Some(a), Some(b)) if comps.is_empty() && a == b => (),
    //             (Some(a), Some(b)) if b == &".".to_string() => comps.push("..".to_string()),
    //             (Some(_), Some(b)) if b == &"..".to_string() => return None,
    //             (Some(a), Some(_)) => {
    //                 comps.push("..".to_string());
    //                 for _ in itb_iter {
    //                     comps.push("..".to_string());
    //                 }
    //                 comps.push(a.to_string());
    //                 break;
    //             }
    //         }
    //     }
    //     // Create the new path from its components 
    //     let mut new_path = String::new();
    //     for component in comps.iter() {
    //         new_path.push_str(&format!("{}/",  component));
    //     }
    //     return Some(Path::new(new_path));
    // }

    /// Gets the reference to the directory specified by the path given the current working directory 
    pub fn get(&self, wd: &StrongDirRef) -> Option<StrongDirRef> {
        let mut new_wd = Arc::clone(&wd);
        for dirname in self.components().iter() {
            // navigate to parent directory
            if dirname == ".." {
                let dir = match new_wd.lock().get_parent_dir() {
                    Some(dir) => dir, 
                    None => return None,
                };
                new_wd = dir;
            }
            // navigate to child directory
            else {
                let dir = match wd.lock().get_child_dir(dirname.to_string()) {
                    Some(dir) => dir, 
                    None => return None,
                };
                new_wd = dir;
            }
        }
        return Some(new_wd);
    }
}