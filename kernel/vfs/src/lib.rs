#![no_std]
#![feature(alloc)]

#[macro_use] extern crate log;
#[macro_use] extern crate alloc;
#[macro_use] extern crate lazy_static;
extern crate spin;

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use alloc::boxed::Box;
use spin::Mutex;
use alloc::arc::{Arc, Weak};

lazy_static! {
    /// The root directory
    pub static ref ROOT: StrongDirRef<VFSDirectory> = {
        let root_dir = VFSDirectory {
            name: "/root".to_string(),
            child_dirs: Vec::new(),
            files: Vec::new(),
            parent: None, 
        };
        Arc::new(Mutex::new(root_dir))
    };
}

pub fn get_root() -> StrongDirRef {
    Arc::clone(&ROOT)
}

/// An strong reference (Arc) and a Mutex wrapper around VFSDirectory
pub type StrongDirRef<D:Directory> = Arc<Mutex<D>>;
pub type StrongRefAnyDirectory = StrongDirRef<Box<Directory + 'static>>;
// type StrongVFSDirectoryRef = StrongDirRef<VFSDirectory>;
/// An weak reference (Weak) and a Mutex wrapper around VFSDirectory
pub type WeakDirRef = Weak<Mutex<Directory>>;

// Traits for files, implementors of File must also implement FileDirectory
pub trait File : FileDirectory {
    fn read(&self);
    fn write(&mut self);
    fn seek(&self); 
    fn delete(&self);
}

/// Traits for directories, implementors of Directory must also implement FileDirectory
pub trait Directory : FileDirectory {
    fn new_dir(&mut self, name: String, parent_pointer:WeakDirRef) -> StrongRefAnyDirectory; 
    fn new_file(&mut self, name: String, parent_pointer: WeakDirRef); 
    fn get_child_dir(&self, child_dir: String) -> Option<StrongRefAnyDirectory>;
    fn get_parent_dir(&self) -> Option<StrongRefAnyDirectory>;
    fn list_children(&mut self) -> String;
    fn get_name(&self) -> String;
}

/// Traits that both files and directories share
pub trait FileDirectory {
    fn get_path_as_string(&self) -> String;
    fn get_path(&self) -> Path;
}

/// A struct that represents a node in the VFS 
pub struct VFSDirectory {
    /// The name of the directory
    name: String,
    /// A list of StrongDirRefs or pointers to the child directories 
    child_dirs: Vec<StrongRefAnyDirectory>,
    /// A list of files within this directory
    files: Vec<VFSFile>,
    /// A weak reference to the parent directory, wrapped in Option because the root directory does not have a parent
    parent: Option<WeakDirRef>,
}

impl Directory for VFSDirectory {
    /// Creates a new directory and passes a reference to the new directory created as output
    fn new_dir(&mut self, name: String, parent_pointer: WeakDirRef) -> StrongRefAnyDirectory {
        let directory = VFSDirectory {
            name: name,
            child_dirs: Vec::new(),
            files:  Vec::new(),
            parent: Some(parent_pointer),
        };
        let dir_ref = Arc::new(Mutex::new(Box::new(directory)));
        self.child_dirs.push(dir_ref.clone());
        dir_ref
    }

    /// Creates a new file with the parent_pointer as the enclosing directory
    fn new_file(&mut self, name: String, parent_pointer: WeakDirRef)  {
        let file = VFSFile {
            name: name,
            size: 0,
            parent: parent_pointer,
        };
        self.files.push(file);
    }
 
    /// Looks for the child directory specified by dirname and returns a reference to it 
    fn get_child_dir(&self, child_dir: String) -> Option<StrongDirRef> {
        for dir in self.child_dirs.iter() {
            if dir.lock().get_name() == child_dir {
                return Some(Arc::clone(dir));
            }
        }
        return None;
    }

    /// Returns a pointer to the parent if it exists
    fn get_parent_dir(&self) -> Option<StrongDirRef> {
        match self.parent {
            Some(parent) => parent.upgrade(),
            None => None
        }
    }

    /// Returns a string listing all the children in the directory
    fn list_children(&mut self) -> String {
        let mut children_list = String::new();
        for dir in self.child_dirs.iter() {
            children_list.push_str(&format!("{}, ",dir.lock().get_name()));
        }

        for file in self.files.iter() {
            children_list.push_str(&format!("{}, ", file.name));
        }
        return children_list;
    }

    fn get_name(&self) -> String {
        self.name
    }
    // TODO - return iterator of children rather than a string
    // fn children(&self) -> Iterator {
    //     let mut children: Vec<&FileDirectory> = Vec::new();
    //     for file in self.files.iter() {
    //         children.push(file);
    //     }
    //     for dir in self.child_dirs.iter() {
    //         children.push(dir);
    //     }
    //     children.iter()
    // }
}

impl FileDirectory for VFSDirectory {
    /// Functions as pwd command in bash, recursively gets the absolute pathname as a String
    fn get_path_as_string(&self) -> String {
        let mut path = self.name.clone();
        if let Some(cur_dir) =  self.get_parent_dir() {
            path.insert_str(0, &format!("{}/",&cur_dir.lock().get_path_as_string()));
            return path;
        }
        return path;
    }
    /// Gets the absolute pathname as a Path struct
    fn get_path(&self) -> Path {
        Path::new(self.get_path_as_string())
    }
}

pub struct VFSFile {
    /// The name of the file
    name: String,
    /// The file size 
    size: usize, 
    /// A weak reference to the parent directory
    parent: WeakDirRef,
}

impl File for VFSFile {
    fn read(&self) { unimplemented!(); }
    fn write(&mut self) { unimplemented!(); }
    fn seek(&self) { unimplemented!(); }
    fn delete(&self) { unimplemented!(); }
}

impl FileDirectory for VFSFile {
    /// Functions as pwd command in bash, recursively gets the absolute pathname as a String
    fn get_path_as_string(&self) -> String {
        let mut path = self.name.clone();
        if let Some(cur_dir) =  self.parent.upgrade() {
            path.insert_str(0, &format!("{}/",&cur_dir.lock().get_path_as_string()));
            return path;
        }
        return path;
    }
    /// Gets the absolute pathname as a Path struct 
    fn get_path(&self) -> Path {
        Path::new(self.get_path_as_string())
    }
}

/// A structure that represents a file path
pub struct Path {
    path: String
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

    /// Returns a canonical and absolute form of the current path
    fn canonicalize(&self, current_path: &Path) -> Path {
        let mut new_components = Vec::new();
        // Push the components of the working directory to the components of the new path
        new_components.extend(current_path.components());
        // Push components of the path to the components of the new path
        for component in self.components().iter() {
            if component == &String::from(".") {
                continue;
            } else if component == &String::from("..") {
                new_components.pop();
            } else {
                new_components.push(component.to_string());
            }
        }
        // Create the new path from its components 
        let mut new_path = String::new();
        for component in new_components.iter() {
            new_path.push_str(&format!("{}/",  component));
        }
        return Path::new(new_path);
    }
    
    /// Expresses the current Path, self, relative to another Path, other
    /// https://docs.rs/pathdiff/0.1.0/src/pathdiff/lib.rs.html#32-74
    pub fn relative(&self, other: &Path) -> Option<Path> {
        let ita = self.components();
        let itb = other.components();
        let mut ita_iter = ita.iter();
        let mut itb_iter = itb.iter();
        let mut comps: Vec<String> = Vec::new();
        loop {
            match (ita_iter.next(), itb_iter.next()) {
                (None, None) => break,
                (Some(a), None) => {
                    comps.push(a.to_string());
                    break;
                }
                (None, _) => comps.push("..".to_string()),
                (Some(a), Some(b)) if comps.is_empty() && a == b => (),
                (Some(a), Some(b)) if b == &".".to_string() => comps.push("..".to_string()),
                (Some(_), Some(b)) if b == &"..".to_string() => return None,
                (Some(a), Some(_)) => {
                    comps.push("..".to_string());
                    for _ in itb_iter {
                        comps.push("..".to_string());
                    }
                    comps.push(a.to_string());
                    break;
                }
            }
        }
        // Create the new path from its components 
        let mut new_path = String::new();
        for component in comps.iter() {
            new_path.push_str(&format!("{}/",  component));
        }
        return Some(Path::new(new_path));
    }

    /// Gets the reference to the directory specified by the path given the current working directory 
    pub fn get(&self, wd: &StrongDirRef) -> Option<StrongDirRef> {
        let current_path = wd.lock().get_path();
        // Get the shortest path from self to working directory by first finding the canonical path then the relative path
        let shortest_path = match self.canonicalize(&current_path).relative(&current_path) {
            Some(dir) => dir,
            None => return None
        };
        let mut new_wd = Arc::clone(&wd);
        for dirname in shortest_path.components().iter() {
            // Navigate to parent directory
            if dirname == ".." {
                let dir = match new_wd.lock().get_parent_dir() {
                    Some(dir) => dir, 
                    None => return None,
                };
                new_wd = dir;
            }
            // Ignore if there is no directory specified at any point in path
            else if dirname == "" {
                continue;
            }
            // Navigate to child directory
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

pub enum PathComponent {
    RootDir,
    ParentDir,
    CurrentDir, 
}

impl PathComponent {
    pub fn as_string(self) -> String {
        match self {
            PathComponent::RootDir => String::from("/root"),
            PathComponent::CurrentDir => String::from("."),
            PathComponent::ParentDir => String::from(".."),
        }
    }
}