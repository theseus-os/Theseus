#![no_std]
#![feature(alloc)]

//! This crate contains an implementation of the virtual mmi dir and file, which 
//! organizes and holds memory management information

// #[macro_use] extern crate log;
extern crate alloc;
extern crate spin;
extern crate fs_node;
extern crate memory;

use alloc::string::String;
use alloc::vec::Vec;
use alloc::boxed::Box;
use spin::Mutex;
use alloc::sync::{Arc, Weak};
use alloc::collections::BTreeMap;
use fs_node::{DirRef, FileRef, WeakDirRef, Directory, FileOrDir, File, FsNode};
use memory::MappedPages;


// /// A struct that represents a node in the VFS 
// pub struct TaskDirectory<'a> {
//     /// The name of the directory
//     pub name: String,
//     // The absolute path of the TaskDirectory
//     path: Path,
//     task: &'a TaskRef,
//     parent: DirRef, // we can store the parent because TaskFs is a persistent directory
// }

// impl<'a> TaskDirectory<'a> {
//     /// Creates a new directory and passes a pointer to the new directory created as output
//     pub fn new(name: String, parent: &DirRef, task: &'a TaskRef)  -> Result<TaskDirectory<'a>, &'static str> {
//         // creates a copy of the parent pointer so that we can add the newly created folder to the parent's children later
//         let task_id = task.lock().id.clone();
//         let directory = TaskDirectory {
//             name: name,
//             path: Path::new(format!("/root/{}/{}", TASKS_DIRECTORY_NAME, task_id)),
//             task: task,
//             parent: Arc::clone(parent),
//         };
//         Ok(directory)
//     }
// }

// impl<'a> Directory for TaskDirectory<'a> {
//     fn insert_child(&mut self, child: FileOrDir, overwrite: bool) -> Result<(), &'static str> {
//         Err("cannot insert child into virtual task directory")
//     }

//     fn get_child(&self, child_name: &str) -> Option<FileOrDir> {
//         None
//         // create a new mmi dir here
//     }

//     /// Returns a string listing all the children in the directory
//     fn list_children(&mut self) -> Vec<String> {
//         let mut children = Vec::new();
//         children.push("mmi".to_string());
//         children
//     }
// }

// impl<'a> FsNode for TaskDirectory<'a> {
//     fn get_name(&self) -> String {
//         self.name.clone()
//     }

//     /// Returns a pointer to the parent if it exists
//     fn get_parent_dir(&self) -> Result<DirRef, &'static str> {
//         Ok(Arc::clone(&self.parent))
//     }
// }



// pub struct TaskFile<'a> {
//     task: &'a TaskRef,
//     path: Path, 
//     parent: WeakDirRef
// }

// impl<'a> TaskFile<'a> {
//     pub fn new(task: &'a TaskRef, parent_dir: &DirRef) -> TaskFile<'a> {
//         let task_id = task.lock().id.clone();
//         TaskFile {
//             task: task,
//             path: Path::new(format!("/root/{}/{}", TASKS_DIRECTORY_NAME, task_id)), 
//             parent: Arc::downgrade(parent_dir)
//         }
//     }

//     /// Generates the task info string.
//     /// TODO: calling format!() and .push_str() is redundant and wastes a lot of allocation. Improve this. 1
//     fn generate(&self) -> String {
//         // Print all tasks
//         let mut task_string = String::new();
//         let name = &self.task.lock().name.clone();
//         let runstate = match &self.task.lock().runstate {
//             RunState::Initing    => "Initing",
//             RunState::Runnable   => "Runnable",
//             RunState::Blocked    => "Blocked",
//             RunState::Reaped     => "Reaped",
//             _                    => "Exited",
//         };
//         let cpu = self.task.lock().running_on_cpu.map(|cpu| format!("{}", cpu)).unwrap_or(String::from("-"));
//         let pinned = &self.task.lock().pinned_core.map(|pin| format!("{}", pin)).unwrap_or(String::from("-"));
//         let task_type = if self.task.lock().is_an_idle_task {"I"}
//         else if self.task.lock().is_application() {"A"}
//         else {" "} ;  

//         task_string.push_str(
//             &format!("{0:<10} {1}\n{2:<10} {3}\n{4:<10} {5}\n{6:<10} {7}\n{8:<10} {9}\n{10:<10} {11:<10}", 
//                 "name", name, "task id",  self.task.lock().id, "runstate", runstate, "cpu",cpu, "pinned", pinned, "task type", task_type)
//         );
        
//         task_string
//     }
// }

// impl<'a> FsNode for TaskFile<'a> {
//     fn get_name(&self) -> String {
//         self.task.lock().name.clone()
//     }

//     /// Returns a pointer to the parent if it exists
//     fn get_parent_dir(&self) -> Result<DirRef, &'static str> {
//         self.parent.upgrade().ok_or("couldn't upgrade parent")
//     }
// }

// impl<'a> File for TaskFile<'a> {
//     fn read(&mut self, buf: &mut [u8]) -> Result<usize, &'static str> { 
//         let output = self.generate();
//         let count = core::cmp::min(buf.len(), output.len());
//         // copy as many bytes as we can 
//         buf[..count].copy_from_slice(&output.as_bytes()[..count]);
//         Ok(count)
//     }

//     fn write(&mut self, buf: &[u8]) -> Result<usize, &'static str> { 
//         Err("not permitted to write task contents through the task VFS") 
//     } 

//     fn delete(self) -> Result<(), &'static str> { 
//         Err("task files are autogenerated, cannot be deleted")
//     }

//     fn size(&self) -> usize { 
//         self.generate().len() 
//     }

//     fn as_mapping(&self) -> Result<&MappedPages, &'static str> {
//         Err("task files are autogenerated, cannot be memory mapped")
//     }
// }

