#![no_std]

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
extern crate io;

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use spin::Mutex;
use alloc::sync::Arc;
use fs_node::{DirRef, WeakDirRef, Directory, FileOrDir, File, FileRef, FsNode};
use memory::MappedPages;
use task::{TaskRef, TASKLIST};
use path::Path;
use io::{ByteReader, ByteWriter, KnownLength, IoError};


/// The name of the VFS directory that exposes task info in the root. 
pub const TASKS_DIRECTORY_NAME: &str = "tasks";
/// The absolute path of the tasks directory, which is currently below the root
pub const TASKS_DIRECTORY_PATH: &str = "/tasks"; 


/// Initializes the tasks virtual filesystem directory within the root directory.
pub fn init() -> Result<(), &'static str> {
    TaskFs::new()?;
    Ok(())
}


/// The top level directory that includes a dynamically-generated list of all `Task`s,
/// each comprising a `TaskDir`.
/// This directory exists in the root directory.
pub struct TaskFs { }

impl TaskFs {
    fn new() -> Result<DirRef, &'static str> {
        let root = root::get_root();
        let dir_ref = Arc::new(Mutex::new(TaskFs { })) as DirRef;
        root.lock().insert(FileOrDir::Dir(dir_ref.clone()))?;
        Ok(dir_ref)
    }

    fn get_self_pointer(&self) -> Option<DirRef> {
        root::get_root().lock().get_dir(&self.get_name())
    }

    fn get_internal(&self, node: &str) -> Result<FileOrDir, &'static str> {
        let id = node.parse::<usize>().map_err(|_e| "could not parse Task id as usize")?;
        let task_ref = task::get_task(id).ok_or("could not get taskref from TASKLIST")?;
        let parent_dir = self.get_self_pointer().ok_or("BUG: tasks directory wasn't in root")?;
        let dir_name = task_ref.id.to_string(); 
        // lazily compute a new TaskDir everytime the caller wants to get a TaskDir
        let task_dir = TaskDir::new(dir_name, &parent_dir, task_ref)?;        
        let boxed_task_dir = Arc::new(Mutex::new(task_dir)) as DirRef;
        Ok(FileOrDir::Dir(boxed_task_dir))
    }
}

impl FsNode for TaskFs {
    fn get_absolute_path(&self) -> String {
        String::from(TASKS_DIRECTORY_PATH)
    }

    fn get_name(&self) -> String {
        String::from(TASKS_DIRECTORY_NAME)
    }

    fn get_parent_dir(&self) -> Option<DirRef> {
        Some(root::get_root().clone())
    }

    fn set_parent_dir(&mut self, _new_parent: WeakDirRef) {
        // do nothing
    }
}

impl Directory for TaskFs {
    /// This function adds a newly created fs node (the argument) to the TASKS directory's children map  
    fn insert(&mut self, _node: FileOrDir) -> Result<Option<FileOrDir>, &'static str> {
        Err("cannot insert node into read-only TaskFs")
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

    fn remove(&mut self, _node: &FileOrDir) -> Option<FileOrDir> {
        None
    }

}




/// A lazily computed directory that holds files and subdirectories related
/// to information about this Task
pub struct TaskDir {
    /// The name of the directory
    pub name: String,
    /// The absolute path of the TaskDir
    path: Path,
    taskref: TaskRef,
    /// We can store the parent (TaskFs) because it is a persistent directory
    parent: DirRef,
}

impl TaskDir {
    /// Creates a new directory and passes a pointer to the new directory created as output
    pub fn new(name: String, parent: &DirRef, taskref: TaskRef)  -> Result<TaskDir, &'static str> {
        let task_id = taskref.id;
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
            return Some(FileOrDir::File(Arc::new(Mutex::new(task_file)) as FileRef));
        }

        if child_name == "mmi" {
            let mmi_dir = MmiDir::new(self.taskref.clone());
            return Some(FileOrDir::Dir(Arc::new(Mutex::new(mmi_dir)) as DirRef));
        }

        None
    }

    /// Returns a string listing all the children in the directory
    fn list(&self) -> Vec<String> {
        let children = vec!["mmi".to_string(), "taskInfo".to_string()];
        children
    }

    fn remove(&mut self, _: &FileOrDir) -> Option<FileOrDir> { 
        None
    }
}

impl FsNode for TaskDir {
    fn get_absolute_path(&self) -> String {
        self.path.clone().into()
    }

    fn get_name(&self) -> String {
        self.name.clone()
    }

    fn get_parent_dir(&self) -> Option<DirRef> {
        Some(self.parent.clone())
    }

    fn set_parent_dir(&mut self, _: WeakDirRef) {
        // do nothing
    }
}



/// Lazily computed file that holds information about this task. This taskfile
/// does not exist witin the actual filesystem. 
pub struct TaskFile {
    taskref: TaskRef,
    task_id: usize,
    path: Path, 
}

impl TaskFile {
    pub fn new(taskref: TaskRef) -> TaskFile {
        let task_id = taskref.id;
        TaskFile {
            taskref,
            task_id,
            path: Path::new(format!("{}/{}/task_info", TASKS_DIRECTORY_PATH, task_id)), 
        }
    }

    /// Generates the task info string.
    fn generate(&self) -> String {
        // Print all tasks
        let cpu = self.taskref.running_on_cpu().map(|cpu| format!("{}", cpu)).unwrap_or_else(|| String::from("-"));
        let pinned = &self.taskref.pinned_core().map(|pin| format!("{}", pin)).unwrap_or_else(|| String::from("-"));
        let task_type = if self.taskref.is_an_idle_task {
            "I"
        } else if self.taskref.is_application() {
            "A"
        } else {
            " "
        };  

        format!("{0:<10} {1}\n{2:<10} {3}\n{4:<10} {5:?}\n{6:<10} {7}\n{8:<10} {9}\n{10:<10} {11:<10}", 
            "name", self.taskref.name,
            "task id", self.taskref.id,
            "runstate", self.taskref.runstate(),
            "cpu", cpu,
            "pinned", pinned,
            "task type", task_type
        )
    }
}

impl FsNode for TaskFile {
    fn get_absolute_path(&self) -> String {
        self.path.clone().into()
    }

    fn get_name(&self) -> String {
        self.taskref.name.clone()
    }

    fn get_parent_dir(&self) -> Option<DirRef> {
        let path = Path::new(format!("{}/{}", TASKS_DIRECTORY_PATH, self.task_id));
        match Path::get_absolute(&path) {
            Some(FileOrDir::Dir(d)) => Some(d),
            _ => None,
        }
    }

    fn set_parent_dir(&mut self, _: WeakDirRef) {
        // do nothing
    }
}

impl ByteReader for TaskFile {
    fn read_at(&mut self, buf: &mut [u8], offset: usize) -> Result<usize, IoError> {
        let output = self.generate();
        if offset > output.len() {
            return Err(IoError::InvalidInput);
        }
        let count = core::cmp::min(buf.len(), output.len() - offset);
        buf[..count].copy_from_slice(&output.as_bytes()[offset..(offset + count)]);
        Ok(count)
    }
}

impl ByteWriter for TaskFile {
    fn write_at(&mut self, _buffer: &[u8], _offset: usize) -> Result<usize, IoError> {
        Err(IoError::from("not permitted to write task contents through the task VFS"))
    } 
    fn flush(&mut self) -> Result<(), IoError> { Ok(()) }
}

impl KnownLength for TaskFile {
    fn len(&self) -> usize {
        self.generate().len() 
    }
}

impl File for TaskFile {
    fn as_mapping(&self) -> Result<&MappedPages, &'static str> {
        Err("task files are autogenerated, cannot be memory mapped")
    }
}






/// Lazily computed directory that contains subfiles and directories 
/// relevant to the task's memory management information. 
pub struct MmiDir {
    taskref: TaskRef,
    task_id: usize,
    path: Path, 
}

impl MmiDir {
    /// Creates a new directory and passes a pointer to the new directory created as output
    pub fn new(taskref: TaskRef) -> MmiDir {
        let task_id = taskref.id;
        MmiDir {
            taskref,
            task_id,
            path: Path::new(format!("{}/{}/mmi", TASKS_DIRECTORY_PATH, task_id)),
        }
    }
}

impl Directory for MmiDir {
    fn insert(&mut self, _node: FileOrDir) -> Result<Option<FileOrDir>, &'static str> {
        Err("cannot insert node into read-only TaskFs")
    }

    fn get(&self, child_name: &str) -> Option<FileOrDir> {
        if child_name == "MmiInfo" {
            // create the new mmi dir here on demand
            let task_file = MmiFile::new(self.taskref.clone());
            Some(FileOrDir::File(Arc::new(Mutex::new(task_file)) as FileRef))
        } else {
            None
        }
    }

    /// Returns a string listing all the children in the directory
    fn list(&self) -> Vec<String> {
        let children = vec!["MmiInfo".to_string()];
        children
    }

    fn remove(&mut self, _: &FileOrDir) -> Option<FileOrDir> {
        None
    }
}

impl FsNode for MmiDir {
    fn get_absolute_path(&self) -> String {
        self.path.clone().into()
    }
    
    fn get_name(&self) -> String {
        "mmi".to_string()
    }

    fn get_parent_dir(&self) -> Option<DirRef> {
        let path = Path::new(format!("{}/{}", TASKS_DIRECTORY_PATH, self.task_id));
        match Path::get_absolute(&path) {
            Some(FileOrDir::Dir(d)) => Some(d),
            _ => None,
        }
    }

    fn set_parent_dir(&mut self, _: WeakDirRef) {
        // do nothing
    }
}



/// Lazily computed file that contains information 
/// about a task's memory management information. 
pub struct MmiFile {
    taskref: TaskRef,
    task_id: usize,
    path: Path, 
}

impl MmiFile {
    pub fn new(taskref: TaskRef) -> MmiFile {
        let task_id = taskref.id;
        MmiFile {
            taskref,
            task_id,
            path: Path::new(format!("{}/{}/mmi/MmiInfo", TASKS_DIRECTORY_PATH, task_id)), 
        }
    }

    /// Generates the mmi info string.
    fn generate(&self) -> String {
        format!("Page table: {:?}\n", self.taskref.mmi.lock().page_table)
    }
}

impl FsNode for MmiFile {
    fn get_absolute_path(&self) -> String {
        self.path.clone().into()
    }

    fn get_name(&self) -> String {
        "MmiInfo".to_string()
    }

    fn get_parent_dir(&self) -> Option<DirRef> {
        let path = Path::new(format!("{}/{}/mmi", TASKS_DIRECTORY_PATH, self.task_id));
        match Path::get_absolute(&path) {
            Some(FileOrDir::Dir(d)) => Some(d),
            _ => None,
        }
    }

    fn set_parent_dir(&mut self, _: WeakDirRef) {
        // do nothing
    }
}

impl ByteReader for MmiFile {
    fn read_at(&mut self, buf: &mut [u8], offset: usize) -> Result<usize, IoError> {
        let output = self.generate();
        if offset > output.len() {
            return Err(IoError::InvalidInput);
        }
        let count = core::cmp::min(buf.len(), output.len() - offset);
        buf[..count].copy_from_slice(&output.as_bytes()[offset..(offset + count)]);
        Ok(count)
    }
}

impl ByteWriter for MmiFile {
    fn write_at(&mut self, _buffer: &[u8], _offset: usize) -> Result<usize, IoError> {
        Err(IoError::from("not permitted to write task contents through the task VFS"))
    } 
    fn flush(&mut self) -> Result<(), IoError> { Ok(()) }
}

impl KnownLength for MmiFile {
    fn len(&self) -> usize {
        self.generate().len() 
    }
}

impl File for MmiFile {
    fn as_mapping(&self) -> Result<&MappedPages, &'static str> {
        Err("task files are autogenerated, cannot be memory mapped")
    }
}

