//! Contains the task implementation of the Directory trait so that each task can be
//! represented as a separate directory within the *tasks* directory. the *tasks* directory
//! is at the root of the filesystem (a direct child of the root directory). Each  *task* within
//! the *tasks* contains information about the task in separate files/subdirectories. 
//! *task* directories are lazily generated, hence the overriding methods of *get_child()* and *list_children()*
//! within the task implementation
use Task;
use TaskRef;
use RunState;
use spin::Mutex;
use alloc::boxed::Box;
use TASKLIST;
use root;
use fs_node::{Directory, File, FsNode, WeakDirRef, DirRef, FileRef, FileOrDir};
use alloc::sync::{Arc, Weak};
use alloc::vec::Vec;
use alloc::string::{String, ToString};
use vfs_node::{VFSDirectory, VFSFile};
use memfs::MemFile;
use path::Path;
use alloc::collections::BTreeMap;
use memory::MappedPages;
use super::task_dir::TaskDirectory;

/// The name of the VFS directory that exposes task info in the root. 
pub const TASKS_DIRECTORY_NAME: &str = "tasks";


/// Initializes the task subfilesystem by creating a directory called task and by creating a file for each task
pub fn init() -> Result<(), &'static str> {
    // let task_fs = root_dir.lock().new("task".to_string(), Arc::downgrade(&root_dir));
    let root = root::get_root();
    let name = String::from(TASKS_DIRECTORY_NAME);
    let task_fs = TaskFs::new(name, &root)?;
    Ok(())
}

pub struct TaskFs {
    name: String,
    /// A list of DirRefs or pointers to the child directories 
    children: BTreeMap<String, FileOrDir>,
    /// A weak reference to the parent directory, wrapped in Option because the root directory does not have a parent
    parent: WeakDirRef,
}

impl TaskFs {
    pub fn new(name: String, parent_dir: &DirRef)  -> Result<DirRef, &'static str> {
        // create a parent copy so that we can add the newly created task directory to the parent's children later
        let parent_copy = Arc::downgrade(parent_dir);
        let directory = TaskFs {
            name: name,
            children: BTreeMap::new(),
            parent: Arc::downgrade(parent_dir),
        };
        let dir_ref = Arc::new(Mutex::new(Box::new(directory) as Box<Directory + Send>));
        let dir_ref_copy = Arc::clone(&dir_ref); // so we can return this copy
        let strong_parent = Arc::clone(parent_dir);
        strong_parent.lock().insert_child(FileOrDir::Dir(dir_ref), false)?;
        Ok(dir_ref_copy)
    }


    fn get_self_pointer(&self) -> Result<DirRef, &'static str> {
        let parent = self.get_parent_dir()?;
        let parent_locked = parent.lock();
        match parent_locked.get_child(&self.get_name()) {
            Some(FileOrDir::Dir(dir)) => Ok(dir),
            _ => Err("BUG: a TaskFile's parent directory didn't contain the TaskFile itself")
        }
    }


    fn get_child_internal(&self, child: &str) -> Result<FileOrDir, &'static str> {
        debug!("ID error is {:?}", child);
        let id = child.parse::<usize>().map_err(|_e| "could not parse usize")?;
        // debug!("ID IS {}", child);
        let task_ref = TASKLIST.get(&id).ok_or("could not get taskref from TASKLIST")?;
        let parent_dir = self.get_self_pointer()?;
        let name = task_ref.lock().id.to_string(); 
        // lazily compute a new TaskDirectory everytime the caller wants to get a TaskDirectory
        let task_dir = TaskDirectory::new(name, &parent_dir, task_ref.clone())?;        
        let boxed_task_dir = Arc::new(Mutex::new(Box::new(task_dir) as Box<Directory + Send>));
        Ok(FileOrDir::Dir(boxed_task_dir))
    }
}

impl FsNode for TaskFs {
    /// Recursively gets the absolute pathname as a String
    fn get_path_as_string(&self) -> String {
        let mut path = String::from(TASKS_DIRECTORY_NAME);
        if let Ok(cur_dir) =  self.get_parent_dir() {
            path.insert_str(0, &format!("{}/",&cur_dir.lock().get_path_as_string()));
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
    fn insert_child(&mut self, child: FileOrDir, overwrite: bool) -> Result<(), &'static str> {
        // gets the name of the child node to be added
        let name = child.get_name();
        if let Some(old_child) = self.children.get(&name) { // the children map contains this key already if this passes
            if overwrite {
                match (old_child, &child) {
                    (FileOrDir::File(_old_file), FileOrDir::Dir(ref _new_file)) => return Err("cannot replace file with directory of same name"),
                    (FileOrDir::Dir(_old_dir), FileOrDir::File(ref _new_dir)) => return Err("cannot replace directory with file of same name"),
                    _ => { } // the types check out, so we can overwrite later
                };
            } else {
                return Err("file or directory with the same name already exists");
            }
        }
        self.children.insert(name, child);
        Ok(())
    }

    fn get_child(&self, child: &str) -> Option<FileOrDir> {
        match self.get_child_internal(child) {
            Ok(d) => Some(d),
            Err(e) => {
                error!("TaskFs::get_child() error: {:?}", e);
                None
            }
        }
    }

    /// Returns a string listing all the children in the directory
    fn list_children(&mut self) -> Vec<String> {
        let mut tasks_string = Vec::new();
        for (id, _taskref) in TASKLIST.iter() {
            tasks_string.push(format!("{}", id));
        }
        tasks_string
    }

}
