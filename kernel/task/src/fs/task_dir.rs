/// Contains the task implementation of the Directory trait so that each task can be
/// represented as a separate directory within the *tasks* directory. the *tasks* directory
/// is at the root of the filesystem (a direct child of the root directory). Each  *task* within
/// the *tasks* contains information about the task in separate files/subdirectories. 
/// *task* directories are lazily generated, hence the overriding methods of *get_child()* and *list_children()*
/// within the task implementation
use Task;
use TaskRef;
use RunState;
use spin::Mutex;
use alloc::boxed::Box;
use TASKLIST;
use root;
use fs_node::{Directory, File, FileDirectory, WeakDirRef, StrongAnyDirRef, FSNode};
use alloc::arc::{Arc, Weak};
use alloc::vec::Vec;
use alloc::string::{String, ToString};
use vfs_node::{VFSDirectory, VFSFile};
use path::Path;


/// Initializes the task subfilesystem by creating a directory called task and by creating a file for each task
pub fn init() -> Result<(), &'static str> {
    // let task_dir = root_dir.lock().new_dir("task".to_string(), Arc::downgrade(&root_dir));
    let root = root::get_root();
    let name = String::from("tasks");
    let task_dir = TaskDirectory::new(name.clone(), Arc::downgrade(&root));
    root.lock().add_fs_node(FSNode::Dir(task_dir))?;
    Ok(())
}

pub struct TaskFile<'a> {
    task: &'a TaskRef,
    path: Path, 
    parent: Option<WeakDirRef>
}

impl<'a> TaskFile<'a> {
    pub fn new(task: &'a TaskRef, parent_pointer: WeakDirRef) -> TaskFile<'a> {
        let task_id = task.lock().id.clone();
        return TaskFile {
            task: task,
            path: Path::new(format!("/root/task/{}", task_id)), 
            parent: Some(parent_pointer)
        };
    }
}

impl<'a> FileDirectory for TaskFile<'a> {
    fn get_path_as_string(&self) -> String {
        return format!("/root/tasks/{}", self.get_name());
    }
    fn get_name(&self) -> String {
        return self.task.lock().name.clone();
    }

        /// Returns a pointer to the parent if it exists
    fn get_parent_dir(&self) -> Option<StrongAnyDirRef> {
        match self.parent {
            Some(ref dir) => dir.upgrade(),
            None => None
        }
    }

    fn get_self_pointer(&self) -> Result<StrongAnyDirRef, &'static str> {
        unimplemented!();
    }

    /// Sets the parent directory of the Task Directory
    /// This function is currently called whenever the VFS root calls add_directory(TaskDirectory)
    /// We should consider making this function private
    fn set_parent(&mut self, parent_pointer: WeakDirRef) {
        self.parent = Some(parent_pointer);
    }
}

impl<'a> File for TaskFile<'a> {
     fn read(&self) -> String { 
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
    
        return task_string;
    }

    fn write(&mut self) { unimplemented!() }
    fn seek(&self) { unimplemented!() }
    fn delete(&self) { unimplemented!() }
}

pub struct TaskDirectory {
    name: String,
    /// A list of StrongDirRefs or pointers to the child directories 
    children: Vec<FSNode>,
    /// A weak reference to the parent directory, wrapped in Option because the root directory does not have a parent
    parent: Option<WeakDirRef>,
}

impl TaskDirectory {
    pub fn new(name: String, parent_pointer: WeakDirRef)  -> StrongAnyDirRef {
        let directory = TaskDirectory {
            name: name,
            children: Vec::new(),
            parent: Some(parent_pointer),
        };
        let dir_ref = Arc::new(Mutex::new(Box::new(directory) as Box<Directory + Send>));
        dir_ref
    }
}

impl FileDirectory for TaskDirectory {
    /// Functions as pwd command in bash, recursively gets the absolute pathname as a String
    fn get_path_as_string(&self) -> String {
        let mut path = String::from("tasks");
        if let Some(cur_dir) =  self.get_parent_dir() {
            path.insert_str(0, &format!("{}/",&cur_dir.lock().get_path_as_string()));
            return path;
        }
        return path;
    }

    fn get_name(&self) -> String {
        return String::from("tasks");
    }

    /// Returns a pointer to the parent if it exists
    fn get_parent_dir(&self) -> Option<StrongAnyDirRef> {
        match self.parent {
            Some(ref dir) => dir.upgrade(),
            None => None
        }
    }

    /// This function returns an Arc<Mutex<>> pointer to a directory by navigating up one directory 
    /// and then cloning itself via the parent's get_child_dir() method
    /// 
    /// Note that this function cannot be used on the root becuase the root doesn't have a parent directory
    fn get_self_pointer(&self) -> Result<StrongAnyDirRef, &'static str> {
        let weak_parent = match self.parent.clone() {
            Some(parent) => parent, 
            None => return Err("could not clone parent")
        };
        let parent = match Weak::upgrade(&weak_parent) {
            Some(weak_ref) => weak_ref,
            None => return Err("could not upgrade parent")
        };

        let mut locked_parent = parent.lock();
        match locked_parent.get_child(self.name.clone(), false) {
            Ok(child) => match child {
                FSNode::File(_file) => return Err("cannot be a file"),
                FSNode::Dir(dir) => return Ok(dir)
            },
            Err(err) => return Err(err),
        }
    }


    /// Sets the parent directory of the Task Directory
    /// This function is currently called whenever the VFS root calls add_directory(TaskDirectory)
    /// We should consider making this function private
    fn set_parent(&mut self, parent_pointer: WeakDirRef) {
        self.parent = Some(parent_pointer);
    }
}

impl Directory for TaskDirectory {
    /// This function adds a newly created fs node (the argument) to the TASKS directory's children vector
    fn add_fs_node(&mut self, new_node: FSNode) -> Result<(), &'static str> {
        match new_node {
            FSNode::Dir(dir) => {
                self.children.push(FSNode::Dir(dir))
                },
            FSNode::File(file) => {
                self.children.push(FSNode::File(file))
                },
        }
        Ok(())
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
            let task_dir = VFSDirectory::new_dir(task_ref.lock().id.to_string(), Arc::downgrade(&parent_pointer)); // this is task 0, 1, etc.
            let task_dir_pointer = Arc::clone(&task_dir);
            self.add_fs_node(FSNode::Dir(Arc::clone(&task_dir))).ok();
            match create_mmi(task_ref.clone(), task_dir_pointer) {
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
fn create_mmi(taskref: TaskRef, task_dir_pointer: StrongAnyDirRef) -> Result<(), &'static str> {
    let mmi_dir: StrongAnyDirRef = VFSDirectory::new_dir(String::from("mmi"), Arc::downgrade(&task_dir_pointer.clone()));
    task_dir_pointer.lock().add_fs_node(FSNode::Dir(mmi_dir.clone()))?;
    // obtain information from the MemoryManagementInfo struct of the Task
    let mut page_table_info = String::from("Virtual Addresses:\n");
    let mmi_info = taskref.lock().mmi.clone().unwrap(); // FIX THIS UNWRAP AND DON'T CLONE
    let vmas = mmi_info.lock().vmas.clone();   
    // gets the start addresses of the virtual memory areas
    for vma in vmas.iter() {
        page_table_info.push_str(&format!("{}\n", vma.start_address()));
    }
    let name = String::from("memoryManagementInfo");
    let mmi_dir_pointer = match mmi_dir.lock().get_self_pointer() {
        Ok(ptr) => ptr, 
        Err(err) => {
            error!("could not obtain pointer to mmi dir because: {}", err);
            return Err(err)
            }
    };
    // create the page table file and add it to the mmi directory
    let page_table_file = VFSFile::new(name.clone(), 0, page_table_info, Some(Arc::downgrade(&mmi_dir_pointer)));
    mmi_dir.lock().add_fs_node(FSNode::File(Arc::new(Mutex::new(Box::new(page_table_file)))))?;
    return Ok(());
}