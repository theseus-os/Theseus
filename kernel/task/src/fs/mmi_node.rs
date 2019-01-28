//! Contains the task implementation of the Directory trait so that each task can be
//! represented as a separate directory within the *tasks* directory. the *tasks* directory
//! is at the root of the filesystem (a direct child of the root directory). Each  *task* within
//! the *tasks* contains information about the task in separate files/subdirectories. 
//! *task* directories are lazily generated, hence the overriding methods of *get_child()* and *list_children()*
//! within the task implementation
use Task;
use TaskRef;
use RunState;
use super::task_fs::TASKS_DIRECTORY_NAME;
use spin::Mutex;
use alloc::boxed::Box;
use TASKLIST;
use root;
use fs_node::{Directory, File, FsNode, WeakDirRef, DirRef, FileRef, FileOrDir};
use alloc::sync::{Arc, Weak};
use alloc::vec::Vec;
use alloc::string::{String, ToString};
use memfs::MemFile;
use path::Path;
use alloc::collections::BTreeMap;
use memory::MappedPages;


/// A struct that represents a node in the VFS 
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
            path: Path::new(format!("/root/{}/{}/mmi", TASKS_DIRECTORY_NAME, task_id)),
            taskref: taskref,
        }
    }
}

impl Directory for MmiDir {
    fn insert_child(&mut self, _child: FileOrDir, _overwrite: bool) -> Result<(), &'static str> {
        Err("cannot insert child into virtual task directory")
    }

    fn get_child(&self, child_name: &str) -> Option<FileOrDir> {
        if child_name == "MmiInfo" {
            let task_file = MmiFile::new(self.taskref.clone());
            return Some(FileOrDir::File(Arc::new(Mutex::new(Box::new(task_file) as Box<File + Send>))));

        }
        None
        // create a new mmi dir here
    }

    /// Returns a string listing all the children in the directory
    fn list_children(&mut self) -> Vec<String> {
        let mut children = Vec::new();
        children.push("MmiInfo".to_string());
        children
    }
}

impl FsNode for MmiDir {
    fn get_name(&self) -> String {
        "mmi".to_string()
    }

    /// Returns a pointer to the parent if it exists
    fn get_parent_dir(&self) -> Result<DirRef, &'static str> {
        // parse from root 
        let path = Path::new(format!("/root/{}/{}", TASKS_DIRECTORY_NAME, self.taskref.lock().id.clone()));
        let dir = match Path::get_from_root(path)? {
            FileOrDir::File(f) => return Err("parent cannot be a file"),
            FileOrDir::Dir(d) => d,
        };
        Ok(dir)
    }
}



/// Note that this MmiFile will reside within the MmiDir, so it will have no concept
/// of a persistent parent directory
pub struct MmiFile {
    taskref: TaskRef,
    path: Path, 
}

impl MmiFile {
    pub fn new(task: TaskRef) -> MmiFile {
        let task_id = task.lock().id.clone();
        MmiFile {
            taskref: task,
            path: Path::new(format!("/root/{}/{}/mmi/MmiInfo", TASKS_DIRECTORY_NAME, task_id)), 
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
        let path = Path::new(format!("/root/{}/{}/mmi", TASKS_DIRECTORY_NAME, self.taskref.lock().id.clone()));
        let dir = match Path::get_from_root(path)? {
            FileOrDir::File(f) => return Err("parent cannot be a file"),
            FileOrDir::Dir(d) => d,
        };
        Ok(dir)
    }
}

impl File for MmiFile {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, &'static str> { 
        let output = self.generate();
        let count = core::cmp::min(buf.len(), output.len());
        // copy as many bytes as we can 
        buf[..count].copy_from_slice(&output.as_bytes()[..count]);
        Ok(count)
    }

    fn write(&mut self, buf: &[u8]) -> Result<usize, &'static str> { 
        Err("not permitted to write task contents through the task VFS") 
    } 

    fn delete(self) -> Result<(), &'static str> { 
        Err("task files are autogenerated, cannot be deleted")
    }

    fn size(&self) -> usize { 
        self.generate().len() 
    }

    fn as_mapping(&self) -> Result<&MappedPages, &'static str> {
        Err("task files are autogenerated, cannot be memory mapped")
    }
}

