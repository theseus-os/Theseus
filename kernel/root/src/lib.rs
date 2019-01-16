#![no_std]
#![feature(alloc)]
//! This crate contains the implementation of the special root directory. The only way that this 
//! directory implementation differs from VFSDirectory is that there is no parent field (becuase the 
//! root has no parent directory), and that internal calls to parent will return some type of error value

#[macro_use] extern crate log;
#[macro_use] extern crate alloc;
#[macro_use] extern crate lazy_static;
extern crate spin;
extern crate fs_node;
extern crate memfs;

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use alloc::boxed::Box;
use spin::Mutex;
use alloc::sync::{Arc, Weak};
use alloc::collections::BTreeMap;
use fs_node::{DirRef, WeakDirRef, Directory, FileOrDir, FsNode, File};

lazy_static! {
    /// The root directory
    /// Returns a tuple for easy access to the name of the root so we don't have to lock it
    pub static ref ROOT: (String, DirRef) = {
        let root_dir = RootDirectory {
            name: "/root".to_string(),
            children: BTreeMap::new() 
        };

        // Creates a file containing the following string so we can test the implementation of MemFile
        let test_string = String::from("TESTINGINMEMORY");
        let strongRoot = Arc::new(Mutex::new(Box::new(root_dir) as Box<Directory + Send>));
        let mut test_bytes =  test_string.as_bytes().to_vec();
        memfs::MemFile::new(String::from("testfile"), &mut test_bytes ,Arc::downgrade(&Arc::clone(&strongRoot))).unwrap();
        (String::from("/root"), strongRoot)

    };
}

pub fn get_root() -> DirRef {
    Arc::clone(&ROOT.1)
}

/// A struct that represents a node in the VFS 
pub struct RootDirectory {
    /// The name of the directory
    name: String,
    /// A list of DirRefs or pointers to the child directories   
    children: BTreeMap<String, FileOrDir>,
}

impl Directory for RootDirectory {
    fn insert_child(&mut self, child: FileOrDir) -> Result<(), &'static str> {
        // gets the name of the child node to be added
        let name = child.get_name();
        self.children.insert(name, child);
        return Ok(())
    }

    fn get_child(&mut self, child_name: String, is_file: bool) -> Result<FileOrDir, &'static str> {
        let option_child = self.children.get(&child_name);
        match option_child {
            Some(child) => match child {
                FileOrDir::File(file) => {
                        return Ok(FileOrDir::File(Arc::clone(file)));
                    }
                FileOrDir::Dir(dir) => {
                        return Ok(FileOrDir::Dir(Arc::clone(dir)));
                    }
            },
            None => Err("could not get child from children map")
        }
    }

    /// Returns a string listing all the children in the directory
    fn list_children(&mut self) -> Vec<String> {
        return self.children.keys().cloned().collect();
    }
}

impl FsNode for RootDirectory {
    /// Recursively gets the absolute pathname as a String
    fn get_path_as_string(&self) -> String {
        return "/root".to_string();
    }

    fn get_name(&self) -> String {
        self.name.clone()
    }

    /// Returns a pointer to the parent if it exists
    fn get_parent_dir(&self) -> Result<DirRef, &'static str> {
        return Err("root does not have a parent");
    }
}
