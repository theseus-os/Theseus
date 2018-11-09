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
    pub static ref ROOT: StrongAnyDirRef = {
        let root_dir = VFSDirectory {
            name: "/root".to_string(),
            child_dirs: Vec::new(),
            files: Vec::new(),
            children: Vec::new(), 
            parent: None, 
        };
        Arc::new(Mutex::new(Box::new(root_dir)))
    };
}

pub fn get_root() -> StrongAnyDirRef {
    Arc::clone(&ROOT)
}

// pub type StrongFileDirRef = Arc<Mutex<Box<FileDirectory + Send>>>;

/// An strong reference (Arc) and a Mutex wrapper around VFSDirectory
pub type StrongDirRef<D: Directory + Send> = Arc<Mutex<D>>;
pub type StrongAnyDirRef = StrongDirRef<Box<Directory + Send>>;
// type StrongVFSDirectoryRef = StrongDirRef<VFSDirectory>;
/// An weak reference (Weak) and a Mutex wrapper around VFSDirectory
pub type WeakDirRef<D: Directory> = Weak<Mutex<D>>;
pub type StrongFileRef = Arc<Mutex<Box<File + Send>>>;

// Traits for files, implementors of File must also implement FileDirectory
pub trait File : FileDirectory {
    fn read(&self) -> String;
    fn write(&mut self);
    fn seek(&self); 
    fn delete(&self);
    fn set_parent(&mut self, parent_pointer: WeakDirRef<Box<Directory + Send>>);
}

/// Traits for directories, implementors of Directory must also implement FileDirectory
pub trait Directory : FileDirectory + Send {
    fn add_directory(&mut self, new_dir: StrongAnyDirRef) -> Result<(), &'static str>;
    fn add_file(&mut self, new_dir: StrongFileRef) -> Result<(), &'static str>;
    fn set_parent(&mut self, parent_pointer: WeakDirRef<Box<Directory + Send>>);
    fn get_child_dir(&self, child_dir: String) -> Option<StrongAnyDirRef>;
    fn get_child_file(&self, child_file: String) -> Option<StrongFileRef>;
    fn get_children(&self) -> Vec<FileDir>;
    fn get_child(&self, child_name: String, is_file: bool) -> Option<FileDir>; 
    fn get_parent_dir(&self) -> Option<StrongAnyDirRef>;
    fn list_children(&mut self) -> Vec<String>;
    fn get_children_files(&self) -> Vec<StrongFileRef>;
    fn get_self_pointer(&self) -> Option<StrongAnyDirRef>;
}

/// Traits that both files and directories share
pub trait FileDirectory {
    fn get_path_as_string(&self) -> String;
    fn get_path(&self) -> Path;
    fn get_name(&self) -> String;
}

pub enum FileDir {
    File(StrongFileRef),
    Dir(StrongAnyDirRef),
}

/// A struct that represents a node in the VFS 
pub struct VFSDirectory {
    /// The name of the directory
    name: String,
    /// A list of StrongDirRefs or pointers to the child directories 
    child_dirs: Vec<StrongAnyDirRef>,
    /// A list of files within this directory
    files: Vec<StrongFileRef>,
    children: Vec<FileDir>,
    /// A weak reference to the parent directory, wrapped in Option because the root directory does not have a parent
    parent: Option<WeakDirRef<Box<Directory + Send>>>,
}

impl VFSDirectory {
    /// Creates a new directory and passes a reference to the new directory created as output
    pub fn new_dir(name: String)  -> StrongAnyDirRef {
        let directory = VFSDirectory {
            name: name,
            child_dirs: Vec::new(),
            files:  Vec::new(),
            children: Vec::new(),
            parent: None,
        };
        let dir_ref = Arc::new(Mutex::new(Box::new(directory) as Box<Directory + Send>));
        dir_ref
    }
}

impl Directory for VFSDirectory {
    fn add_directory(&mut self, new_dir: StrongAnyDirRef) -> Result<(), &'static str> {
        let self_pointer = match self.get_self_pointer() {
            Some(self_ptr) => self_ptr,
            None => return Err("Couldn't obtain pointer to self")
        };
        new_dir.lock().set_parent(Arc::downgrade(&self_pointer));
        self.child_dirs.push(new_dir);
        Ok(())
    }

    fn add_file(&mut self, new_file: StrongFileRef) -> Result<(), &'static str> {
        let self_pointer = match self.get_self_pointer() {
            Some(self_ptr) => self_ptr,
            None => return Err("Couldn't obtain pointer to self")
        };
        new_file.lock().set_parent(Arc::downgrade(&self_pointer));
        self.files.push(new_file);
        Ok(())
    }

    fn set_parent(&mut self, parent_pointer: WeakDirRef<Box<Directory + Send>>) {
        self.parent = Some(parent_pointer);
    }
 
    /// Looks for the child directory specified by dirname and returns a reference to it 
    fn get_child_dir(&self, child_dir: String) -> Option<StrongAnyDirRef> {
        for dir in self.child_dirs.iter() {
            if dir.lock().get_name() == child_dir {
                return Some(Arc::clone(dir));
            }
        }
        return None;
    }

    fn get_child(&self, child_name: String, is_file: bool) -> Option<FileDir> {
        for child in self.children.iter() {
            match child {
                FileDir::File(file) => {
                    if file.lock().get_name() == child_name && is_file { 
                        return Some(FileDir::File(Arc::clone(file)));
                    }
                }
                FileDir::Dir(dir) => {
                    if dir.lock().get_name() == child_name && !is_file { 
                        return Some(FileDir::Dir(Arc::clone(dir)));
                    }
                }
            }
        }
        return None;
    }

    fn get_child_file(&self, child_file: String) -> Option<StrongFileRef> {
        for file in self.files.iter() {
            if file.lock().get_name() == child_file {
                return Some(Arc::clone(file));
            }
        }
        return None;
    }

    /// Returns a pointer to the parent if it exists
    fn get_parent_dir(&self) -> Option<StrongAnyDirRef> {
        match self.parent {
            Some(ref dir) => dir.upgrade(),
            None => None
        }
    }

    /// Returns a string listing all the children in the directory
    fn list_children(&mut self) -> Vec<String> {
        let mut children_list = Vec::new();
        for dir in self.child_dirs.iter() {
            children_list.push(format!("{}\n",dir.lock().get_name()));
        }

        for file in self.files.iter() {
            children_list.push(format!("{}\n", file.lock().get_name()));
        }
        return children_list;
    }
    
    fn get_children(&self) -> Vec<FileDir> {
        return self.children;
    }

    // TODO - return iterator of children rather than a string
    fn get_children_files(&self) -> Vec<StrongFileRef> {
        let mut children: Vec<StrongFileRef> = Vec::new();
        for file in self.files.iter() {
            children.push(file.clone());
        }
        children
    }

    fn get_self_pointer(&self) -> Option<StrongAnyDirRef> {
        if self.parent.is_none() {
            debug!("fix this jank ass shit later");
            return Some(get_root());
        }

        debug!("in get self pointer");
        let weak_parent = match self.parent.clone() {
            Some(parent) => parent, 
            None => return None
        };
        debug!("got weak parent");
        let parent = match Weak::upgrade(&weak_parent) {
            Some(weak_ref) => weak_ref,
            None => return None
        };
        debug!("about to return");
        return parent.lock().get_child_dir(self.name.clone());
    }
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
    fn get_name(&self) -> String {
        self.name.clone()
    }
}

pub struct VFSFile {
    /// The name of the file
    name: String,
    /// The file size 
    size: usize, 
    /// A weak reference to the parent directory
    parent: WeakDirRef<Box<Directory + Send>>,
}

impl File for VFSFile {
    fn read(&self) -> String { unimplemented!(); }
    fn write(&mut self) { unimplemented!(); }
    fn seek(&self) { unimplemented!(); }
    fn delete(&self) { unimplemented!(); }
    fn set_parent(&mut self, parent_pointer: WeakDirRef<Box<Directory + Send>>) {
        self.parent = parent_pointer;
    }
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
    fn get_name(&self) -> String {
        self.name.clone()
    }
}

/// A structure that represents a file path
#[derive(Debug, Clone)]
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
    pub fn components(&self) -> Vec<String> {
        let components = self.path.split("/").map(|s| s.to_string()).filter(|x| x != "").collect();
        return components;
    } 

    /// Returns a canonical and absolute form of the current path (i.e. the path of the working directory)
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
        debug!("canonical {}", new_path.clone());
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
                    for remaining_a in ita_iter {
                        comps.push(remaining_a.to_string());
                    }
                    break;
                }
                (None, _) => comps.push("..".to_string()),
                (Some(a), Some(b)) if comps.is_empty() && a == b => continue,
                (Some(a), Some(b)) if b == &".".to_string() => comps.push("..".to_string()),
                (Some(_), Some(b)) if b == &"..".to_string() => return None,
                (Some(a), Some(_)) => {
                    comps.push("..".to_string());
                    for _ in itb_iter {
                        comps.push("..".to_string());
                    }
                    comps.push(a.to_string());
                    for remaining_a in ita_iter {
                        comps.push(remaining_a.to_string());
                    }
                    break;
                }
            }
        }
        // Create the new path from its components 
        let mut new_path = String::new();
        for component in comps.iter() {
            new_path.push_str(&format!("{}/",  component));
        }
        debug!("relative {}", new_path.clone());
        return Some(Path::new(new_path));
    }

    /// Gets the reference to the directory specified by the path given the current working directory 
    pub fn get(&self, wd: &StrongAnyDirRef) -> Option<FileDir> {
        let current_path;
        { current_path = wd.lock().get_path();}
        debug!("current path {}", current_path.path);
        // Get the shortest path from self to working directory by first finding the canonical path of self then the relative path of that path to the 
        let shortest_path = match self.canonicalize(&current_path).relative(&current_path) {
            Some(dir) => dir, 
            None => return None
        };
        let mut new_wd = Arc::clone(&wd);
        debug!("components {:?}", shortest_path.components());
        let mut counter: isize = -1;
        for component in shortest_path.components().iter() {
            counter += 1; 
            // Navigate to parent directory
            if component == ".." {
                let dir = match new_wd.lock().get_parent_dir() {
                    Some(dir) => dir, 
                    None => return None,
                };
                new_wd = dir;
            }
            // Ignore if no directory is specified 
            else if component == "" {
                continue;
            }

            // Navigate to child directory
            else {
                // this checks the last item in the components to check if it's a file
                // if no matching file is found, advances to the next match block
                if counter as usize == shortest_path.components().len() - 1  && shortest_path.components()[0] != ".." { // FIX LATER
                    debug!("entered third else");
                    let files = new_wd.lock().list_children(); // fixes this so that it uses list_children so we don't preemptively create a bunch of TaskFile objects
                    debug!("new wd is not deadlocked");
                    for file_name in files.iter() {
                            // debug!("file ref name: {}", file_ref.lock().get_name());
                            // debug!("c2omponent name: {}", component.to_string());
                        if file_name == component {
                            let child_file = match new_wd.lock().get_child_file(file_name.to_string()) {
                                Some(file) => file,
                                None => return None,
                            };                        
                            return Some(FileDir::File(Arc::clone(&child_file)));
                        }
                    }
                }
                let dir = match new_wd.lock().get_child_dir(component.to_string()) {
                    Some(dir) => dir, 
                    None => return None,
                };
                new_wd = dir;
            }

        }
        
    

        return Some(FileDir::Dir(new_wd));
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