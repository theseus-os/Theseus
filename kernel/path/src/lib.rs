#![no_std]
#![feature(alloc)]
/// This crate contains all the necessary functions for navigating the virtual filesystem / obtaining specific
/// directories via the Path struct 
#[macro_use] extern crate log;
#[macro_use] extern crate alloc;
extern crate spin;
extern crate fs_node;
extern crate vfs_node;

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use alloc::arc::Arc;
use fs_node::{FSNode, StrongAnyDirRef};

/// A structure that represents a file  
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
    pub fn get(&self, wd: &StrongAnyDirRef) -> Result<FSNode, &'static str> {
        let current_path;
        { current_path = Path::new(wd.lock().get_path_as_string());}
        
        // Get the shortest path from self to working directory by first finding the canonical path of self then the relative path of that path to the 
        let shortest_path = match self.canonicalize(&current_path).relative(&current_path) {
            Some(dir) => dir, 
            None => {
                error!("cannot canonicalize path {}", current_path.path); 
                return Err("couldn't canonicalize path");
            }
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
                    None => {
                        error!("failed to move up in path {}", current_path.path);
                        return Err("could not move up in path")
                        }, 
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
                    let children = new_wd.lock().list_children(); // fixes this so that it uses list_children so we don't preemptively create a bunch of TaskFile objects
                    for child_name in children.iter() {
                        if child_name == component {
                            match new_wd.lock().get_child(child_name.to_string(), false) {
                                Ok(child) => match child {
                                    FSNode::File(file) => return Ok(FSNode::File(Arc::clone(&file))),
                                    FSNode::Dir(dir) => {
                                        return Ok(FSNode::Dir(Arc::clone(&dir)));
                                    }
                                },
                                Err(err) => return Err(err),
                            };                       
                        }
                    }
                }
                               
                let dir = match new_wd.lock().get_child(component.clone().to_string(),  false) {
                    Ok(child) => match child {
                        FSNode::Dir(dir) => dir,
                        FSNode::File(_file) => return Err("shouldn't be a file here"),
                    }, 
                    Err(err) => return Err(err),
                };
                new_wd = dir;
            }

        }
        return Ok(FSNode::Dir(new_wd));
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