#![no_std]
#![feature(alloc)]

#[macro_use] extern crate log;
#[macro_use] extern crate alloc;


use alloc::string::{String, ToString};
use alloc::vec::Vec;


pub struct File {
    name: String, 
    filepath: String,
    size: usize, 
    filetype: FileType,
}

pub enum FileType {
    test, 
    text,
}


impl File {

    pub fn read(self) -> String {
        return format!("name: {}, filepath: {}, size: {}, filetype: {}", self.name, self.filepath, self.size, String::from("temp filetype"));
    }
}


pub struct Directory {
    name: String,
    child_dirs: Vec<Directory>,
    files: Vec<File>,
}


impl Directory {
    /// Creates the root directory
    pub fn create_root() -> Directory {
        let root = Directory {
            name: "root".to_string(),
            child_dirs: Vec::new(),
            files: Vec::new(),
        };
        
        return root;
    }


    /// Assumes you actually want to open the file upon creation
    pub fn new_file(mut self, name: String, filepath: String, filetype: FileType) {
        let file = File {
            name: name,
            filepath: filepath,
            size: 0,
            filetype: filetype
        };

        self.files.push(file);
    }   

    pub fn new_dir(mut self, name: String) {
        let directory = Directory {
            name: name, 
            child_dirs: Vec::new(),
            files:  Vec::new(),
        };
        self.child_dirs.push(directory);
    }


    pub fn list_children(self) -> String {
        let mut children_list = String::new();
        for dir in self.child_dirs.iter() {
            children_list.push_str(&format!("{}, ",dir.name.to_string()));
        }

        for file in self.files.iter() {
            children_list.push_str(&format!("{}, ", file.name.to_string()));
        }
        return children_list;
    }




}

pub fn hack_loop(dir: Directory) {
    loop { }
}