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

/// The name of the VFS directory that exposes task info in the root. 
pub const TASKS_DIRECTORY_NAME: &str = "tasks";


/// Initializes the task subfilesystem by creating a directory called task and by creating a file for each task
pub fn init() -> Result<(), &'static str> {
    // let task_dir = root_dir.lock().new("task".to_string(), Arc::downgrade(&root_dir));
    let root = root::get_root();
    let name = String::from(TASKS_DIRECTORY_NAME);
    let task_dir = TaskDirectory::new(name, Arc::downgrade(&root))?;
    Ok(())
}

pub struct TaskFile<'a> {
    task: &'a TaskRef,
    path: Path, 
    parent: WeakDirRef
}

impl<'a> TaskFile<'a> {
    pub fn new(task: &'a TaskRef, parent_dir: WeakDirRef) -> TaskFile<'a> {
        let task_id = task.lock().id.clone();
        return TaskFile {
            task: task,
            path: Path::new(format!("{}/{}/{}", root::ROOT_DIRECTORY_NAME, TASKS_DIRECTORY_NAME, task_id)), 
            parent: parent_dir
        };
    }

    /// Generates the task info string.
    /// TODO: calling format!() and .push_str() is redundant and wastes a lot of allocation. Improve this. 
    fn generate(&self) -> String {
        // Print all tasks
        let mut task_string = String::new();
        let name = &self.task.lock().name.clone();
        let runstate = match &self.task.lock().runstate {
            RunState::Initing    => "Initing",
            RunState::Runnable   => "Runnable",
            RunState::Blocked    => "Blocked",
            RunState::Reaped     => "Reaped",
            _                    => "Exited",
        };
        let cpu = self.task.lock().running_on_cpu.map(|cpu| format!("{}", cpu)).unwrap_or(String::from("-"));
        let pinned = &self.task.lock().pinned_core.map(|pin| format!("{}", pin)).unwrap_or(String::from("-"));
        let task_type = if self.task.lock().is_an_idle_task {"I"}
        else if self.task.lock().is_application() {"A"}
        else {" "} ;  

        task_string.push_str(
            &format!("{0:<10} {1}\n{2:<10} {3}\n{4:<10} {5}\n{6:<10} {7}\n{8:<10} {9}\n{10:<10} {11:<10}", 
                "name", name, "task id",  self.task.lock().id, "runstate", runstate, "cpu",cpu, "pinned", pinned, "task type", task_type)
        );
        
        task_string
    }
}

impl<'a> FsNode for TaskFile<'a> {
    fn get_name(&self) -> String {
        return self.task.lock().name.clone();
    }

    /// Returns a pointer to the parent if it exists
    fn get_parent_dir(&self) -> Result<DirRef, &'static str> {
        self.parent.upgrade().ok_or("couldn't upgrade parent")
    }
}

impl<'a> File for TaskFile<'a> {
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

pub struct TaskDirectory {
    name: String,
    /// A list of DirRefs or pointers to the child directories 
    children: BTreeMap<String, FileOrDir>,
    /// A weak reference to the parent directory, wrapped in Option because the root directory does not have a parent
    parent: WeakDirRef,
}

impl TaskDirectory {
    pub fn new(name: String, parent_dir: WeakDirRef)  -> Result<DirRef, &'static str> {
        // create a parent copy so that we can add the newly created task directory to the parent's children later
        let parent_copy = Weak::clone(&parent_dir);
        let directory = TaskDirectory {
            name: name,
            children: BTreeMap::new(),
            parent: parent_dir,
        };
        let dir_ref = Arc::new(Mutex::new(Box::new(directory) as Box<Directory + Send>));
        let dir_ref_copy = Arc::clone(&dir_ref); // so we can return this copy
        let strong_parent = Weak::upgrade(&parent_copy).ok_or("could not upgrade parent pointer")?;
        strong_parent.lock().insert_child(FileOrDir::Dir(dir_ref))?;
        return Ok(dir_ref_copy);
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
        let id = child.parse::<usize>().map_err(|_e| "could not parse usize")?;
        let task_ref = super::super::get_task(id).ok_or("could not get taskref from TASKLIST")?;
        let parent_dir = match self.get_self_pointer() {
            Ok(ptr) => ptr, 
            Err(err) => {
                error!("could not get self because: {}", err);
                return Err(err)
            },
        };

        // We have to violate orthogonality to avoid a locking issue only present because calling tasks.lock().get_child()
        // locks the highest-level tasks directory, which would then be locked again if we called the regular VFSDirectory::new() method
        // We'll manually create the VFSDirectory instead and add it right here
        let name = task_ref.lock().id.to_string(); 
        let new_dir = VFSDirectory {
            name: name.clone(),
            children: BTreeMap::new(),
            parent: Arc::downgrade(&parent_dir),
        };
        let task_dir = Arc::new(Mutex::new(Box::new(new_dir) as Box<Directory + Send>));
        create_mmi_dir(task_ref, &task_dir)?;
        Ok(FileOrDir::Dir(task_dir))
    }
}

impl FsNode for TaskDirectory {
    /// Recursively gets the absolute pathname as a String
    fn get_path_as_string(&self) -> String {
        let mut path = String::from(TASKS_DIRECTORY_NAME);
        if let Ok(cur_dir) =  self.get_parent_dir() {
            let parent_path = &cur_dir.lock().get_path_as_string();
            // Check if the parent path is root
            if parent_path == "/" {
                path.insert_str(0, &format!("{}", parent_path));
                return path;
            }
            path.insert_str(0, &format!("{}/", parent_path));
            return path;
            
        }
        return path;
    }

    fn get_name(&self) -> String {
        return String::from(TASKS_DIRECTORY_NAME);
    }

    /// Returns a pointer to the parent if it exists
    fn get_parent_dir(&self) -> Result<DirRef, &'static str> {
        self.parent.upgrade().ok_or("couldn't upgrade parent")
    }
}

impl Directory for TaskDirectory {
    /// This function adds a newly created fs node (the argument) to the TASKS directory's children vector    
    fn insert_child(&mut self, child: FileOrDir) -> Result<(), &'static str> {
        // gets the name of the child node to be added
        let name = child.get_name();
        self.children.insert(name, child);
        return Ok(())
    }

    fn get_child(&self, child: &str) -> Option<FileOrDir> {
        match self.get_child_internal(child) {
            Ok(d) => Some(d),
            Err(e) => {
                error!("TaskDirectory::get_child() error: {:?}", e);
                None
            }
        }
    }

    /// Returns a string listing all the children in the directory
    fn list_children(&mut self) -> Vec<String> {
        let mut tasks_string = Vec::new();
        for (id, _taskref) in TASKLIST.lock().iter() {
            tasks_string.push(format!("{}", id));
        }
        tasks_string
    }

}

/// Creates the memory management info subdirectory of a task directory.
/// This function will attach the mmi directory (and associated subdirectories) to whatever directory task_dir_pointer points to.
fn create_mmi_dir(taskref: TaskRef, parent: &DirRef) -> Result<(), &'static str> {
    let name = String::from("mmi");
    let mmi_dir = VFSDirectory::new(name.clone(), parent)?;
    // obtain information from the MemoryManagementInfo struct of the Task
    let mut output = String::new();
    match taskref.lock().mmi {
        Some(ref mmi) => {
            output.push_str(&format!("Page table:\n{:?}\n", mmi.lock().page_table));
            output.push_str(&format!("Virtual memory areas:\n{:?}", mmi.lock().vmas));
        }
        _ => output.push_str("MMI was None."),
    }
    // create the actual MMI file and add it to the mmi directory
    let _page_table_file = MemFile::new(name, output.as_bytes(), &mmi_dir);
    Ok(())
}