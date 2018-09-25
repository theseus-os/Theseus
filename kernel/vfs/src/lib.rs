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
    pub static ref ROOT: StrongDirRef = {
        let root_dir = Directory {
            basename: "/root".to_string(),
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

pub type StrongDirRef = Arc<Mutex<Directory>>;
pub type WeakDirRef = Weak<Mutex<Directory>>;

pub struct Directory{
    pub basename: String,
    /// The absolute path of the file from the root
    path: String,
    child_dirs: Vec<StrongDirRef>,
    files: Vec<File>,
    parent: WeakDirRef,
}


impl Directory {
    /// Assumes you actually want to open the file upon creation
    pub fn new_file(&mut self, name: String, filepath: String, parent_pointer: WeakDirRef)  {
        let file = File {
            basename: name,
            filepath: filepath,
            size: 0,
            parent: parent_pointer,
        };
        self.files.push(file);
    }

    /// Creates a new directory and passes a reference to the new directory created as output
    pub fn new_dir(&mut self, name: String, parent_pointer: WeakDirRef) -> StrongDirRef {
        let temp_name = name.clone();
        let directory = Directory {
            basename: name, 
            path: format!("{}/{}", self.path, temp_name.clone()),
            child_dirs: Vec::new(),
            files:  Vec::new(),
            parent: parent_pointer,
        };
        let dir_ref = Arc::new(Mutex::new(directory));
        self.child_dirs.push(dir_ref.clone());
        dir_ref
    }
    
    /// Looks for the child directory specified by dirname and returns a reference to it 
    pub fn get_child_dir(&self, dirname: String) -> Option<StrongDirRef> {
        for dir in self.child_dirs.iter() {
            if dir.lock().basename == dirname {
                return Some(Arc::clone(dir));
            }
        }
        return None;
    }
    /// Returns a string listing all the children in the directory
    pub fn list_children(&mut self) -> String {
        let mut children_list = String::new();
        for dir in self.child_dirs.iter() {
            children_list.push_str(&format!("{}, ",dir.lock().basename.to_string()));
        }

        for file in self.files.iter() {
            children_list.push_str(&format!("{}, ", file.basename.to_string()));
        }
        return children_list;
    }
    
    /// Functions as pwd command in bash
    pub fn get_path(&self) -> String {
        return self.path.clone();
    }
}

pub struct File {
    basename: String, 
    filepath: String,
    size: usize, 
    parent: WeakDirRef,
}

impl File {
     pub fn read(self) -> String {
        return format!("name: {}, filepath: {}, size: {}, filetype: {}", self.basename, self.filepath, self.size, String::from("temp filetype"));
    }
}