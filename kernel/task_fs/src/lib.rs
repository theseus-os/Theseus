#![no_std]
#![feature(alloc)]

//! This crate contains the direcotires and files that comprise the taskfs, which is similar
//! to the /proc directory in linux. There are four main sections in this code:
//! 1) TaskFs: the top level directory that holds the individual TaskDirs
//! 2) TaskDir: the lazily computed directory that contains files and directories 
//!     relevant to that task
//! 3) TaskFile: lazily computed file that holds information about the task
//! 4) MmiDir: lazily computed directory that holds subdirectories and files
//!     about the task's memory management information
//! 5) MmiFile: lazily computed file that contains information about the task's
//!     memory management information
//! 
//! * Note that all the structs here are NOT persistent in the filesystem EXCEPT
//! for the TaskFs struct, which contains all the individual TaskDirs. This means 
//! that when a terminal cd's into a TaskDir or one of the subdirectories, it is the 
//! only entity that has a reference to that directory. When the terminal drops that 
//! reference (i.e. backs out of the directory), that directory is dropped from scope
//! 
//! The hierarchy (tree) is as follows:
//! 
//!             TaskDir
//!         TaskFile    MmiDir
//!                         MmiFile
//! 

#[macro_use] extern crate alloc;
#[macro_use] extern crate log;

extern crate spin;
extern crate fs_node;
extern crate memory;
extern crate task;
extern crate path;
extern crate root;


use alloc::string::{String, ToString};
use alloc::vec::Vec;
use alloc::boxed::Box;
use spin::Mutex;
use alloc::sync::{Arc, Weak};
use alloc::collections::BTreeMap;
use fs_node::{DirRef, FileRef, WeakDirRef, Directory, FileOrDir, File, FsNode};
use memory::MappedPages;
use task::{TaskRef, TASKLIST, RunState};
use path::Path;


/// The name of the VFS directory that exposes task info in the root. 
pub const TASKS_DIRECTORY_NAME: &str = "tasks";
/// The absolute path of the tasks directory, which is currently below the root
pub const TASKS_DIRECTORY_PATH: &str = "/tasks"; 

/// Initializes the task subfilesystem by creating a directory called task and by creating a file for each task
pub fn init() -> Result<(), &'static str> {
    // let task_fs = root_dir.lock().new("task".to_string(), Arc::downgrade(&root_dir));
    let root = root::get_root();
    let name = String::from(TASKS_DIRECTORY_NAME);
    let task_fs = TaskFs::new(name, &root)?;
    Ok(())
}


/// The top level directory that is analagous to Linux's /proc directory. Contains
/// all the individual TaskDirs. This directory is actually persistent within the 
/// filesystem. 
pub struct TaskFs {
    name: String,
    /// A weak reference to the parent directory, wrapped in Option because the root directory does not have a parent
    parent: WeakDirRef,
}

impl TaskFs {
    pub fn new(name: String, parent_dir: &DirRef)  -> Result<DirRef, &'static str> {
        // create a parent copy so that we can add the newly created task directory to the parent's children later
        let directory = TaskFs {
            name: name,
            parent: Arc::downgrade(parent_dir),
        };
        let dir_ref = Arc::new(Mutex::new(Box::new(directory) as Box<Directory + Send>));
        let dir_ref_copy = Arc::clone(&dir_ref); // so we can return this copy
        let strong_parent = Arc::clone(parent_dir);
        strong_parent.lock().insert(FileOrDir::Dir(dir_ref))?;
        Ok(dir_ref_copy)
    }


    fn get_self_pointer(&self) -> Result<DirRef, &'static str> {
        let parent = self.get_parent_dir()?;
        let parent_locked = parent.lock();
        match parent_locked.get(&self.get_name()) {
            Some(FileOrDir::Dir(dir)) => Ok(dir),
            _ => Err("BUG: a TaskFile's parent directory didn't contain the TaskFile itself")
        }
    }


    fn get_internal(&self, node: &str) -> Result<FileOrDir, &'static str> {
        let id = node.parse::<usize>().map_err(|_e| "could not parse usize")?;
        let task_ref = task::get_task(id).ok_or("could not get taskref from TASKLIST")?;
        let parent_dir = self.get_self_pointer()?;
        let name = task_ref.lock().id.to_string(); 
        // lazily compute a new TaskDir everytime the caller wants to get a TaskDir
        let task_dir = TaskDir::new(name, &parent_dir, task_ref.clone())?;        
        let boxed_task_dir = Arc::new(Mutex::new(Box::new(task_dir) as Box<Directory + Send>));
        Ok(FileOrDir::Dir(boxed_task_dir))
    }
}

impl FsNode for TaskFs {
    /// Recursively gets the absolute pathname as a String
    fn get_absolute_path(&self) -> String {
        let mut path = String::from(TASKS_DIRECTORY_NAME);
        if let Ok(cur_dir) =  self.get_parent_dir() {
            path.insert_str(0, &format!("{}",&cur_dir.lock().get_absolute_path()));
        }
        path
    }

    fn get_name(&self) -> String {
        String::from(TASKS_DIRECTORY_NAME)
    }

    /// Returns a pointer to the parent if it exists
    fn get_parent_dir(&self) -> Result<DirRef, &'static str> {
        self.parent.upgrade().ok_or("couldn't upgrade parent")
    }
}

impl Directory for TaskFs {
    /// This function adds a newly created fs node (the argument) to the TASKS directory's children map  
    fn insert(&mut self, node: FileOrDir) -> Result<Option<FileOrDir>, &'static str> {
        Err("TaskFs is read-only")
    }

    fn get(&self, node: &str) -> Option<FileOrDir> {
        match self.get_internal(node) {
            Ok(d) => Some(d),
            Err(e) => {
                error!("TaskFs::get() error: {:?}", e);
                None
            }
        }
    }

    /// Returns a string listing all the children in the directory
    fn list(&self) -> Vec<String> {
        let mut tasks_string = Vec::new();
        for (id, _taskref) in TASKLIST.lock().iter() {
            tasks_string.push(format!("{}", id));
        }
        tasks_string
    }

    fn remove(&mut self, _node: &FileOrDir) -> Result<(), &'static str> {
        Err("cannot remove nodes from read-only TaskFs")
    }

}




/// A lazily computed directory that holds files and subdirectories related
/// to information about this Task
pub struct TaskDir {
    /// The name of the directory
    pub name: String,
    // The absolute path of the TaskDir
    path: Path,
    taskref: TaskRef,
    parent: DirRef, // we can store the parent because TaskFs is a persistent directory
}

impl TaskDir {
    /// Creates a new directory and passes a pointer to the new directory created as output
    pub fn new(name: String, parent: &DirRef, taskref: TaskRef)  -> Result<TaskDir, &'static str> {
        // creates a copy of the parent pointer so that we can add the newly created folder to the parent's children later
        let task_id = taskref.lock().id.clone();
        let directory = TaskDir {
            name: name,
            path: Path::new(format!("{}/{}", TASKS_DIRECTORY_PATH, task_id)),
            taskref: taskref,
            parent: Arc::clone(parent),
        };
        Ok(directory)
    }
}

impl Directory for TaskDir {
    fn insert(&mut self, _node: FileOrDir) -> Result<Option<FileOrDir>, &'static str> {
        Err("cannot insert node into read-only TaskFs")
    }

    fn get(&self, child_name: &str) -> Option<FileOrDir> {
        if child_name == "taskInfo" {
            let task_file = TaskFile::new(self.taskref.clone());
            return Some(FileOrDir::File(Arc::new(Mutex::new(Box::new(task_file) as Box<File + Send>))));

        }

        if child_name == "mmi" {
            let mmi_dir = MmiDir::new(self.taskref.clone());
            return Some(FileOrDir::Dir(Arc::new(Mutex::new(Box::new(mmi_dir) as Box<Directory + Send>))));
            
        }
        None
    }

    /// Returns a string listing all the children in the directory
    fn list(&self) -> Vec<String> {
        let mut children = Vec::new();
        children.push("mmi".to_string());
        children.push("taskInfo".to_string());
        children
    }

    fn remove(&mut self, _node: &FileOrDir) -> Result<(), &'static str> {
        Err("cannot remove node from read-only TaskFs")
    }
}

impl FsNode for TaskDir {
    fn get_name(&self) -> String {
        self.name.clone()
    }

    /// Returns a pointer to the parent if it exists
    fn get_parent_dir(&self) -> Result<DirRef, &'static str> {
        Ok(Arc::clone(&self.parent))
    }
}



/// Lazily computed file that holds information about this task. This taskfile
/// does not exist witin the actual filesystem. 
pub struct TaskFile {
    taskref: TaskRef,
    path: Path, 
}

impl TaskFile {
    pub fn new(task: TaskRef) -> TaskFile {
        let task_id = task.lock().id.clone();
        TaskFile {
            taskref: task,
            path: Path::new(format!("{}/{}/task_info", TASKS_DIRECTORY_PATH, task_id)), 
        }
    }

    /// Generates the task info string.
    /// TODO: calling format!() and .push_str() is redundant and wastes a lot of allocation. Improve this. 1
    fn generate(&self) -> String {
        // Print all tasks
        let mut task_string = String::new();
        let name = &self.taskref.lock().name.clone();
        let runstate = match &self.taskref.lock().runstate {
            RunState::Initing    => "Initing",
            RunState::Runnable   => "Runnable",
            RunState::Blocked    => "Blocked",
            RunState::Reaped     => "Reaped",
            _                    => "Exited",
        };
        let cpu = self.taskref.lock().running_on_cpu.map(|cpu| format!("{}", cpu)).unwrap_or(String::from("-"));
        let pinned = &self.taskref.lock().pinned_core.map(|pin| format!("{}", pin)).unwrap_or(String::from("-"));
        let task_type = if self.taskref.lock().is_an_idle_task {"I"}
        else if self.taskref.lock().is_application() {"A"}
        else {" "} ;  

        task_string.push_str(
            &format!("{0:<10} {1}\n{2:<10} {3}\n{4:<10} {5}\n{6:<10} {7}\n{8:<10} {9}\n{10:<10} {11:<10}", 
                "name", name, "task id",  self.taskref.lock().id, "runstate", runstate, "cpu",cpu, "pinned", pinned, "task type", task_type)
        );
        
        task_string
    }
}

impl FsNode for TaskFile {
    fn get_name(&self) -> String {
        self.taskref.lock().name.clone()
    }

    /// Returns a pointer to the parent if it exists
    fn get_parent_dir(&self) -> Result<DirRef, &'static str> {
        // parse from root 
        let path = Path::new(format!("{}/{}", TASKS_DIRECTORY_PATH, self.taskref.lock().id.clone()));
        let dir = match Path::get_absolute(&path)? {
            FileOrDir::File(f) => return Err("parent cannot be a file"),
            FileOrDir::Dir(d) => d,
        };
        Ok(dir)
    }
}

impl File for TaskFile {
    fn read(&self, buf: &mut [u8], offset: usize) -> Result<usize, &'static str> { 
        let output = self.generate();
        let count = core::cmp::min(buf.len(), output.len());
        // copy as many bytes as we can 
        buf[..count].copy_from_slice(&output.as_bytes()[..count]);
        Ok(count)
    }

    fn write(&mut self, buf: &[u8], offset: usize) -> Result<usize, &'static str> { 
        Err("not permitted to write task contents through the task VFS") 
    } 

    fn size(&self) -> usize { 
        self.generate().len() 
    }

    fn as_mapping(&self) -> Result<&MappedPages, &'static str> {
        Err("task files are autogenerated, cannot be memory mapped")
    }
}






/// Lazily computed directory that contains subfiles and directories 
/// relevant to the task's memory management information. 
pub struct MmiDir {
    // The absolute path of the MmiDir
    path: Path,
    taskref: TaskRef,
}

impl MmiDir {
    /// Creates a new directory and passes a pointer to the new directory created as output
    pub fn new(taskref: TaskRef)  -> MmiDir {
        // creates a copy of the parent pointer so that we can add the newly created folder to the parent's children later
        let task_id = taskref.lock().id.clone();
        MmiDir {
            path: Path::new(format!("{}/{}/mmi", TASKS_DIRECTORY_PATH, task_id)),
            taskref: taskref,
        }
    }
}

impl Directory for MmiDir {
    fn insert(&mut self, _node: FileOrDir) -> Result<Option<FileOrDir>, &'static str> {
        Err("cannot insert node into read-only TaskFs")
    }

    fn get(&self, child_name: &str) -> Option<FileOrDir> {
        if child_name == "MmiInfo" {
            let task_file = MmiFile::new(self.taskref.clone());
            return Some(FileOrDir::File(Arc::new(Mutex::new(Box::new(task_file) as Box<File + Send>))));

        }
        None
        // create a new mmi dir here
    }

    /// Returns a string listing all the children in the directory
    fn list(&self) -> Vec<String> {
        let mut children = Vec::new();
        children.push("MmiInfo".to_string());
        children
    }

    fn remove(&mut self, _node: &FileOrDir) -> Result<(), &'static str> {
        Err("cannot remove node from read-only TaskFs")
    }
}

impl FsNode for MmiDir {
    fn get_name(&self) -> String {
        "mmi".to_string()
    }

    /// Returns a pointer to the parent if it exists
    fn get_parent_dir(&self) -> Result<DirRef, &'static str> {
        // parse from root 
        let path = Path::new(format!("{}/{}", TASKS_DIRECTORY_PATH, self.taskref.lock().id.clone()));
        let dir = match Path::get_absolute(&path)? {
            FileOrDir::File(f) => return Err("parent cannot be a file"),
            FileOrDir::Dir(d) => d,
        };
        Ok(dir)
    }
}



/// Lazily computed file that contains information about this task's memory
/// management information. 
pub struct MmiFile {
    taskref: TaskRef,
    path: Path, 
}

impl MmiFile {
    pub fn new(task: TaskRef) -> MmiFile {
        let task_id = task.lock().id.clone();
        MmiFile {
            taskref: task,
            path: Path::new(format!("{}/{}/mmi/MmiInfo", TASKS_DIRECTORY_PATH, task_id)), 
        }
    }

    /// Generates the mmi info string.
    /// TODO: calling format!() and .push_str() is redundant and wastes a lot of allocation. Improve this. 1
    fn generate(&self) -> String {
        let mut output = String::new();
        match self.taskref.lock().mmi {
            Some(ref mmi) => {
                output.push_str(&format!("Page table:\n{:?}\n", mmi.lock().page_table));
                output.push_str(&format!("Virtual memory areas:\n{:?}", mmi.lock().vmas));
            }
            _ => output.push_str("MMI was None."),
        }
        output
    }
}

impl FsNode for MmiFile {
    fn get_name(&self) -> String {
        // self.taskref.lock().name.clone()
        "MmiInfo".to_string()
    }

    /// Returns a pointer to the parent if it exists
    fn get_parent_dir(&self) -> Result<DirRef, &'static str> {
        // parse from root 
        let path = Path::new(format!("{}/{}/mmi", TASKS_DIRECTORY_PATH, self.taskref.lock().id.clone()));
        let dir = match Path::get_absolute(&path)? {
            FileOrDir::File(f) => return Err("parent cannot be a file"),
            FileOrDir::Dir(d) => d,
        };
        Ok(dir)
    }
}

impl File for MmiFile {
    fn read(&self, buf: &mut [u8], offset: usize) -> Result<usize, &'static str> { 
        let output = self.generate();
        let count = core::cmp::min(buf.len(), output.len());
        // copy as many bytes as we can 
        buf[..count].copy_from_slice(&output.as_bytes()[..count]);
        Ok(count)
    }

    fn write(&mut self, buf: &[u8], offset: usize) -> Result<usize, &'static str> { 
        Err("not permitted to write task contents through the task VFS") 
    } 

    fn size(&self) -> usize { 
        self.generate().len() 
    }

    fn as_mapping(&self) -> Result<&MappedPages, &'static str> {
        Err("task files are autogenerated, cannot be memory mapped")
    }
}

