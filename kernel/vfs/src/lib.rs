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
            name: "/root".to_string(),
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


// fn test() {
//     let dir_pointer = StrongDirRef;
//     let parent_pointer = Arc::clone(dir_pointer);5
//     dir_pointer.lock().new_dir("shit", Arc::downgrade(parent_pointer));
// }


pub struct Directory{
    name: String,
    path: String,
    child_dirs: Vec<StrongDirRef>,
    files: Vec<File>,
    parent: WeakDirRef,
}


impl Directory {
    /// Assumes you actually want to open the file upon creation
    pub fn new_file(&mut self, name: String, filepath: String, parent_pointer: WeakDirRef) {
        let file = File {
            name: name,
            filepath: filepath,
            size: 0,
            parent: parent_pointer,
        };
        self.files.push(file);
    }   

    pub fn new_dir(&mut self, name: String, parent_pointer: WeakDirRef) {
        let temp_name = name.clone();
        let directory = Directory {
            name: name, 
            path: format!("{}/{}", self.path, temp_name.clone()),
            child_dirs: Vec::new(),
            files:  Vec::new(),
            parent: parent_pointer,
        };

        self.child_dirs.push(Arc::new(Mutex::new(directory)));
    }


    pub fn list_children(&mut self) -> String {
        let mut children_list = String::new();
        for dir in self.child_dirs.iter() {
            children_list.push_str(&format!("{}, ",dir.lock().name.to_string()));
        }

        for file in self.files.iter() {
            children_list.push_str(&format!("{}, ", file.name.to_string()));
        }
        return children_list;
    }

    /// Functions as pwd command in bash
    pub fn get_path(&self) -> String {
        return self.path.clone();
    }
}



pub struct File {
    name: String, 
    filepath: String,
    size: usize, 
    parent: WeakDirRef,
}


impl File {
    pub fn read(self) -> String {
        return format!("name: {}, filepath: {}, size: {}, filetype: {}", self.name, self.filepath, self.size, String::from("temp filetype"));
    }
}
