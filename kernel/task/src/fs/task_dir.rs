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
use fs_node::{Directory, File, FSCompatible, WeakDirRef, DirRef, FileRef, FSNode};
use alloc::sync::{Arc, Weak};
use alloc::vec::Vec;
use alloc::string::{String, ToString};
use vfs_node::{VFSDirectory, VFSFile};
use memfs::MemFile;
use path::Path;
use alloc::collections::BTreeMap;

pub const TASKS_STR: &str = "tasks";

/// Initializes the task subfilesystem by creating a directory called task and by creating a file for each task
pub fn init() -> Result<(), &'static str> {
    // let task_dir = root_dir.lock().new_dir("task".to_string(), Arc::downgrade(&root_dir));
    let root = root::get_root();
    let name = String::from(TASKS_STR);
    let task_dir = TaskDirectory::new(name, Arc::downgrade(&root))?;
    Ok(())
}

pub struct TaskFile<'a> {
    task: &'a TaskRef,
    path: Path, 
    parent: WeakDirRef
}

impl<'a> TaskFile<'a> {
    pub fn new(task: &'a TaskRef, parent_pointer: WeakDirRef) -> TaskFile<'a> {
        let task_id = task.lock().id.clone();
        return TaskFile {
            task: task,
            path: Path::new(format!("/root/{}/{}", TASKS_STR, task_id)), 
            parent: parent_pointer
        };
    }
}

impl<'a> FSCompatible for TaskFile<'a> {
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
    
        // return task_string;
        return Ok(0); // temporary, need to fix
    }

    fn write(&mut self, buf: &[u8]) -> Result<usize, &'static str> { Err("not permitted to write task contents through the task VFS") } 
    fn seek(&self) { unimplemented!() }
    fn delete(self) { unimplemented!() }
    fn size(&self) -> usize { 0 }
}

pub struct TaskDirectory {
    name: String,
    /// A list of DirRefs or pointers to the child directories 
    children: BTreeMap<String, FSNode>,
    /// A weak reference to the parent directory, wrapped in Option because the root directory does not have a parent
    parent: WeakDirRef,
}

impl TaskDirectory {
    pub fn new(name: String, parent_pointer: WeakDirRef)  -> Result<DirRef, &'static str> {
        // create a parent copy so that we can add the newly created task directory to the parent's children later
        let parent_copy = Weak::clone(&parent_pointer);
        let directory = TaskDirectory {
            name: name,
            children: BTreeMap::new(),
            parent: parent_pointer,
        };
        let dir_ref = Arc::new(Mutex::new(Box::new(directory) as Box<Directory + Send>));
        let dir_ref_copy = Arc::clone(&dir_ref); // so we can return this copy
        let strong_parent = Weak::upgrade(&parent_copy).ok_or("could not upgrade parent pointer")?;
        strong_parent.lock().insert_child(FSNode::Dir(dir_ref))?;
        return Ok(dir_ref_copy);
    }

    fn add_fs_node(&mut self, new_node: FSNode) -> Result<(), &'static str> {
        let name = new_node.get_name();
        match new_node {
            FSNode::Dir(dir) => {
                self.children.insert(name, FSNode::Dir(dir));
                },
            FSNode::File(file) => {
                self.children.insert(name, FSNode::File(file));
                },
        }
        Ok(())
    }

    fn get_self_pointer(&self) -> Result<DirRef, &'static str> {
        let parent = match self.get_parent_dir() {
            Ok(parent) => parent,
            Err(err) => return Err(err)
        };

        let mut locked_parent = parent.lock();
        match locked_parent.get_child(self.get_name(), false) {
            Ok(child) => {
                match child {
                    FSNode::Dir(dir) => Ok(dir),
                    FSNode::File(_file) => Err("should not be a file"),
                }
            },
            Err(err) => return Err(err)
        }
    }
}

impl FSCompatible for TaskDirectory {
    /// Recursively gets the absolute pathname as a String
    fn get_path_as_string(&self) -> String {
        let mut path = String::from(TASKS_STR);
        if let Ok(cur_dir) =  self.get_parent_dir() {
            path.insert_str(0, &format!("{}/",&cur_dir.lock().get_path_as_string()));
            return path;
        }
        return path;
    }

    fn get_name(&self) -> String {
        return String::from(TASKS_STR);
    }

    /// Returns a pointer to the parent if it exists
    fn get_parent_dir(&self) -> Result<DirRef, &'static str> {
        self.parent.upgrade().ok_or("couldn't upgrade parent")
    }
}

impl Directory for TaskDirectory {
    /// This function adds a newly created fs node (the argument) to the TASKS directory's children vector    
    fn insert_child(&mut self, child: FSNode) -> Result<(), &'static str> {
        // gets the name of the child node to be added
        let name = child.get_name();
        self.children.insert(name, child);
        return Ok(())
    }

    fn get_child(&mut self, child: String, is_file: bool) -> Result<FSNode, &'static str> {
        if is_file {
            return Err("cannot get a file from the tasks directory");
        } 
        else {
            let id = match child.parse::<usize>() {
                Ok(id) => id, 
                Err(_err) => {
                    return Err("could not parse usize");
                    },
            };
            let task_ref = match TASKLIST.get(&id)  {
                Some(task_ref) => task_ref,
                None => return Err("could not get taskref from TASKLIST"),
            };
            let parent_pointer = match self.get_self_pointer() {
                Ok(ptr) => ptr, 
                Err(err) => {
                        error!("could not get self because: {}", err);
                        return Err(err)
                    },
            };

            // We have to violate orthogonality to avoid a locking issue only present because calling tasks.lock().get_child()
            // locks the highest-level tasks directory, which would then be locked again if we called the regular VFSDirectory::new() method
            // We'll manually create the VFSDirectory instead and add it right here
            let new_dir = VFSDirectory {
                name: task_ref.lock().id.to_string(),
                children: BTreeMap::new(),
                parent: Arc::downgrade(&parent_pointer),
            };
            let task_dir = Arc::new(Mutex::new(Box::new(new_dir) as Box<Directory + Send>));
            let task_dir_pointer = Arc::clone(&task_dir);
            self.add_fs_node(FSNode::Dir(Arc::clone(&task_dir))).ok();
            match create_mmi_dir(task_ref.clone(), task_dir_pointer) {
                Ok(mmi_info) => {
                    mmi_info
                }, 
                Err(err) => return Err(err)
            };
            return Ok(FSNode::Dir(task_dir));
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

/// Creates the memory management info subdirectory of a task directory.
/// This function will attach the mmi directory (and associated subdirectories) to whatever directory task_dir_pointer points to.
fn create_mmi_dir(taskref: TaskRef, task_dir_pointer: DirRef) -> Result<(), &'static str> {
    let mmi_dir: DirRef = VFSDirectory::new_dir(String::from("mmi"), Arc::downgrade(&task_dir_pointer.clone()))?;
    // obtain information from the MemoryManagementInfo struct of the Task
    let mut page_table_info = String::from("Virtual Addresses:\n");
    let mmi_info = taskref.lock().mmi.clone().unwrap(); // FIX THIS UNWRAP AND DON'T CLONE
    let vmas = mmi_info.lock().vmas.clone();   
    // gets the start addresses of the virtual memory areas
    for vma in vmas.iter() {
        page_table_info.push_str(&format!("{}\n", vma.start_address()));
    }
    let name = String::from("memoryManagementInfo");
    let mmi_dir_ptr_copy = Arc::clone(&mmi_dir);
    // create the page table file and add it to the mmi directory
    let mut page_table_info = page_table_info.as_bytes().to_vec();
    let _page_table_file = MemFile::new(name.clone(), &mut page_table_info, Arc::downgrade(&mmi_dir_ptr_copy));
    return Ok(());
}