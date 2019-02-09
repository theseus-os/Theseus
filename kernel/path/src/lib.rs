#![no_std]
#![feature(alloc)]
/// This crate contains all the necessary functions for navigating the virtual filesystem / obtaining specific
/// directories via the Path struct 
#[macro_use] extern crate log;
#[macro_use] extern crate alloc;
extern crate spin;
extern crate fs_node;
extern crate root;

use core::fmt;
use core::ops::{Deref, DerefMut};
use alloc::{
    string::{String, ToString},
    vec::Vec,
    sync::Arc,
};
use fs_node::{FileOrDir, DirRef};

pub const PATH_DELIMITER: &str = "/";
pub const EXTENSION_DELIMITER: &str = ".";


/// A structure that represents a file  
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct Path {
    path: String
}

impl Deref for Path {
    type Target = String;

    fn deref(&self) -> &String {
        &self.path
    }
}
impl DerefMut for Path {
    fn deref_mut(&mut self) -> &mut String {
        &mut self.path
    }
}

impl fmt::Display for Path {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        write!(f, "{}", self.path)
    }
}

impl Path {
    /// Creates a new `Path` from the given String.
    pub fn new(path: String) -> Self {
        Path { path }
    }
    
    /// Returns an iterator over the components of this `Path`,
    /// split by the path delimiter `"/"`.
    pub fn components<'a>(&'a self) -> impl Iterator<Item = &'a str> {
        self.path.split(PATH_DELIMITER)
            .filter(|&x| x != "")
    }

    /// Returns just the file name, i.e., the trailling component of the path.
    /// # Examples
    /// `"/path/to/me/file.a"` -> "file.a"
    /// `"me/file.a"` -> "file.a"
    /// `"file.a"` -> "file.a"
    pub fn basename<'a>(&'a self) -> &'a str {
        self.components()
            .last()
            .unwrap_or_else(|| &self.path)
    }

    /// Like [`basename()`](#method.basename), but excludes the file extension, if present.
    pub fn file_stem<'a>(&'a self) -> &'a str {
        self.basename()
            .split(EXTENSION_DELIMITER)
            .filter(|&x| x != "")
            .next()
            .unwrap_or_else(|| &self.path)
    }

    /// Returns the file extension, if present. 
    /// If there are multiple extensions as defined by the extension delimiter, `'.'`,
    /// then the last one will be treated as the extension. 
    pub fn extension<'a>(&'a self) -> Option<&'a str> {
        self.basename()
            .split(EXTENSION_DELIMITER)
            .filter(|&x| x != "")
            .last()
    }

    /// Returns a canonical and absolute form of the current path (i.e. the path of the working directory)
    fn canonicalize(&self, current_path: &Path) -> Path {
        debug!("SELF PATH BEFORE CANONICALIZATION: {}", self.path);
        debug!("ARG PATH BEFORE CANONICALIZATION: {}", current_path.path);

        let mut new_components = Vec::new();
        // Push the components of the working directory to the components of the new path
        new_components.extend(current_path.components());
        // Push components of the path to the components of the new path
        for component in self.components() {
            if component == String::from(".") {
                continue;
            } else if component == String::from("..") {
                new_components.pop();
            } else {
                new_components.push(component);
            }
        }
        // Create the new path from its components 
        let mut new_path = String::new();
        let mut first_cmpnt = true; 
        for component in new_components {
            if first_cmpnt {
                new_path.push_str(&format!("{}",  component));
                first_cmpnt = false;
            } 
            else {
                new_path.push_str(&format!("/{}",  component));
            }
        }
        debug!("NEW PATH IS {}", new_path);
        Path::new(new_path)
    }
    
    /// Expresses the current Path, self, relative to another Path, other
    /// https://docs.rs/pathdiff/0.1.0/src/pathdiff/lib.rs.html#32-74
    pub fn relative(&self, other: &Path) -> Option<Path> {
        let mut ita_iter = self.components();
        let mut itb_iter = other.components();
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
                (Some(ref a), Some(ref b)) if comps.is_empty() && a == b => continue,
                (Some(ref _a), Some(ref b)) if b == &".".to_string() => comps.push("..".to_string()),
                (Some(_), Some(ref b)) if b == &"..".to_string() => return None,
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
        // Remove the trailing slash after the final path component
        new_path.pop();
        Some(Path::new(new_path))
    }
    
    /// Returns a boolean indicating whether this Path is absolute,
    /// i.e., whether it starts with the root directory.
    pub fn is_absolute(&self) -> bool {
        self.path.starts_with(PATH_DELIMITER)
    }

    /// Returns the file or directory specified by the given path, 
    /// which can either be absolute, or relative from the given the current working directory 
    pub fn get(&self, starting_dir: &DirRef) -> Result<FileOrDir, &'static str> {
        let shortest_path = Path::new(self.path.clone());
        let current_path = { Path::new(starting_dir.lock().get_path_as_string()) };
        let mut curr_dir = {
            if self.is_absolute() {
                Arc::clone(root::get_root())
            }
            else {
                Arc::clone(&starting_dir)
            }
        };

        for component in shortest_path.components() {
            match component {
                "." => { 
                    // stay in the current directory, do nothing. 
                }
                ".." => {
                    // navigate to parent directory
                    let parent_dir = curr_dir.lock().get_parent_dir().map_err(|_e| {
                        error!("Path::get(): failed to move up to parent dir, path {}", current_path);
                        "failed to move up to parent dir"
                    })?;
                    curr_dir = parent_dir;
                }
                cmpnt => {
                    // navigate to child directory, or return the child file
                    let child_dir = match curr_dir.lock().get_child(cmpnt) {
                        Some(FileOrDir::File(f)) => return Ok(FileOrDir::File(f)),
                        Some(FileOrDir::Dir(d)) => d,
                        None => return Err("file or directory not found"),
                    };
                    curr_dir = child_dir;
                }
            }
        }
        Ok(FileOrDir::Dir(curr_dir))
    }


    /// Returns the file or directory specified by the given absolute path
    pub fn get_absolute(path: &Path) -> Result<FileOrDir, &'static str> {
        if path.is_absolute() {
            path.get(root::get_root())
        } else {
            Err("given path was not absolute")
        }
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
            PathComponent::RootDir => String::from(root::ROOT_DIRECTORY_NAME),
            PathComponent::CurrentDir => String::from("."),
            PathComponent::ParentDir => String::from(".."),
        }
    }
}